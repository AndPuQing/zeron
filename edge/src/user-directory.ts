/**
 * UserDirectory — ONE Durable Object (`users1`) recording every WorkOS user
 * (name, email, profile picture, when they joined: WorkOS `created_at`) and
 * every organization with its members.
 *
 * Fed by WorkOS webhooks (`user.*`, `organization.*` and
 * `organization_membership.*`: created, updated, deleted) at
 * `POST /workos/webhook`, signed with the endpoint's secret
 * (`WORKOS_WEBHOOK_SECRET`). Deliveries can arrive out of order and retry,
 * so a write lands only when its `updated_at` is at least the stored one;
 * a delete leaves a tombstone (id, joined, deleted) with the profile cleared,
 * and nothing revives it.
 *
 * Membership rows name a user and an org, not directory rows, so they can
 * land before either. `GET /admin/organizations` joins them on read: a member
 * shows the user's current profile, and a member whose user or org was
 * deleted drops out however the deliveries were ordered.
 *
 * Every user gets a user number (#1, #2, …) the first time they're recorded:
 * stored, never changed or reused, and kept by a tombstone. Users recorded
 * before numbers existed were numbered once, in join order.
 *
 * `GET /admin/users[?format=csv]` and `GET /admin/organizations[?format=csv]`
 * (ADMIN_TOKEN) read the tables back.
 *
 * Badges: an operator defines a badge once (`POST /admin/badges`, id like
 * `cloud-tester`) and grants or revokes it for users
 * (`POST /admin/badges/{grant|revoke}`). A new badge is a new row, not new
 * code. Grants name a user id, not a directory row, so a user can hold a
 * badge before their first webhook lands.
 *
 * Org creations: each user creates at most one org (`POST /auth/orgs`).
 * `org_creations` holds one row per user: a claim taken before WorkOS is
 * called, so concurrent requests can't both create, and then the org it made.
 * A failed create gives its claim back; a claim older than
 * `ORG_CLAIM_STALE_MS` that never got an org was left by a create that died
 * mid-way, and the next request takes it over. The admin lists show who
 * created which org.
 *
 * Invitations (`POST /auth/orgs/{id}/invitations`): `invite_quota` and
 * `invite_quota_day` count the addresses each user sent in the current hour
 * and day, `INVITES_PER_HOUR` and `INVITES_PER_DAY` at most.
 * Step-ups (step-up.ts): `step_ups` holds the hash of each live sign-in
 * ticket, its user and expiry; a deletion drops a user's.
 * Images (`PUT /auth/me/images`): `user_images` counts each user's against
 * `MAX_IMAGES_PER_USER`, `MAX_IMAGE_BYTES_PER_USER` and
 * `IMAGE_UPLOADS_PER_DAY`, and names what goes with them (user-images.ts).
 * Org locks: `org_locks` serialises role changes, removals and admins'
 * deletions per org (`lockOrg`), so an org is never left without an admin.
 * Onboarding (`/onboarding`, onboarding-routes.ts): `onboarding_done` holds
 * each page a user has finished and when they first did, at most
 * `MAX_ONBOARDING_PAGES` per user.
 * Plugin keys (`/plugin-keys`, plugin-keys.ts): `plugin_keys` binds each
 * plugin signing key (raw ed25519, b64url) to the account that registered
 * it, at most `PLUGIN_KEYS_PER_DAY` new ones a user a day. Revoked rows stay,
 * so a key is never re-bound; deleting a user revokes theirs.
 *
 * Settings (auth-routes.ts): `PATCH /auth/me` writes the renamed user with the
 * webhook's own `upsert` right away, and an org's member list reads names,
 * pictures through `profiles`.
 *
 * Writes are one row per sign-up or profile change, so a single instance is
 * not a hotspot.
 */
import { DurableObject } from "cloudflare:workers";
import type { Env } from "./env";
import { json, jsonError } from "./http";
import { adminAuthorized } from "./cloud/billing";
import { deleteUserBlobs } from "./user-images";

export const USER_DIRECTORY_NAME = "users1";

/** A new row's user number. Conflict updates never set `user_number`, so a
 * number is taken only on first insert. */
const NEXT_USER_NUMBER = "(SELECT COALESCE(MAX(user_number), 0) + 1 FROM users)";

/** How long an org-creation claim without an org holds before another
 * request may take it over. */
export const ORG_CLAIM_STALE_MS = 5 * 60_000;

/** Invitation addresses one user may send an hour, and in 24 hours. */
export const INVITES_PER_HOUR = 20;
export const INVITES_PER_DAY = 50;
/** Onboarding pages stored per user. */
export const MAX_ONBOARDING_PAGES = 500;
/** New plugin signing keys one user may register in 24 hours. */
export const PLUGIN_KEYS_PER_DAY = 20;
/** Images (`PUT /auth/me/images`) one user may hold, their bytes together,
 * and how many they may put in 24 hours (deleted ones count). */
export const MAX_IMAGES_PER_USER = 100;
export const MAX_IMAGE_BYTES_PER_USER = 20_000_000;
export const IMAGE_UPLOADS_PER_DAY = 200;
/** How long an org's role lock holds before it lapses (a request that died). */
export const ORG_LOCK_MS = 30_000;

/** The fields of a WorkOS user object this directory keeps (wire names). */
export interface WorkOsUser {
  readonly id: string;
  readonly email: string;
  readonly first_name: string | null;
  readonly last_name: string | null;
  readonly profile_picture_url: string | null;
  readonly created_at: string;
  readonly updated_at: string;
}

export interface DirectoryUser {
  readonly userId: string;
  /** 1 for the first user recorded; assigned once, never reused. */
  readonly userNumber: number;
  readonly email: string | null;
  readonly name: string | null;
  readonly firstName: string | null;
  readonly lastName: string | null;
  readonly profilePictureUrl: string | null;
  /** ISO 8601, WorkOS `created_at`. */
  readonly joinedAt: string;
  readonly updatedAt: string;
  readonly deletedAt: string | null;
  /** Badges held, oldest grant first. */
  readonly badges: readonly UserBadge[];
  /** Onboarding pages finished, first finished first. */
  readonly onboardingDone: readonly string[];
  /** The org they created through `POST /auth/orgs`, if any. */
  readonly createdOrgId: string | null;
}

export interface Badge {
  /** Stable id, e.g. `cloud-tester`. */
  readonly id: string;
  readonly name: string;
  readonly description: string | null;
  /** Free-form icon name for clients to map; null = their default. */
  readonly icon: string | null;
  /** Epoch ms the badge was first defined. */
  readonly createdAt: number;
  /** How many users hold it. */
  readonly holders: number;
}

export interface UserBadge {
  readonly id: string;
  /** Epoch ms. */
  readonly grantedAt: number;
  readonly note: string | null;
}

/** The fields of a WorkOS organization object this directory keeps. */
export interface WorkOsOrganization {
  readonly id: string;
  readonly name: string;
  readonly external_id: string | null;
  /** Domain names only; WorkOS's objects also carry verification state. */
  readonly domains: readonly string[];
  readonly created_at: string;
  readonly updated_at: string;
}

/** The fields of a WorkOS organization membership this directory keeps. */
export interface WorkOsMembership {
  readonly id: string;
  readonly organization_id: string;
  readonly user_id: string;
  /** Role slug, e.g. `admin`. */
  readonly role: string | null;
  /** `active`, `inactive` or `pending`. */
  readonly status: string;
  readonly created_at: string;
  readonly updated_at: string;
}

/** `claimOrgCreation`: go ahead under `claimId`, or the user already created
 * `organizationId` (null while their first create is still running). */
export type OrgCreationClaim = { readonly claimId: string } | { readonly organizationId: string | null };

export interface OrganizationMember {
  readonly membershipId: string;
  readonly userId: string;
  /** Null until the user's own webhook lands, as is the rest of the profile. */
  readonly userNumber: number | null;
  readonly email: string | null;
  readonly name: string | null;
  readonly profilePictureUrl: string | null;
  readonly role: string | null;
  readonly status: string;
  /** ISO 8601, when the membership was created. */
  readonly joinedAt: string;
}

export interface DirectoryOrganization {
  readonly orgId: string;
  readonly name: string | null;
  readonly externalId: string | null;
  readonly domains: readonly string[];
  /** ISO 8601, WorkOS `created_at`. */
  readonly createdAt: string;
  readonly updatedAt: string;
  readonly deletedAt: string | null;
  /** Who created it through `POST /auth/orgs`; null for orgs made otherwise
   * or before that was recorded. */
  readonly createdBy: string | null;
  /** Active members; `members` also lists inactive and pending ones. */
  readonly memberCount: number;
  /** Oldest membership first. */
  readonly members: readonly OrganizationMember[];
}

/** `profiles`: what an org's member list shows of a user. */
export interface MemberProfile {
  readonly email: string | null;
  readonly firstName: string | null;
  readonly lastName: string | null;
  readonly profilePictureUrl: string | null;
  readonly username: string | null;
  /** What they do, under their name (`PATCH /auth/me {title}`); null for none. */
  readonly title: string | null;
  /** Their own photo's key in BLOBS (`avatars/{key}`); null for none. */
  readonly photoKey: string | null;
}

/** `registerPluginKey`'s answer. */
export type PluginKeyRegistration =
  | "registered"
  | "key_taken"
  | "key_revoked"
  | "deleted_user"
  | "rate_limited"
  | "step_up_required";

/** `GET /plugin-keys/:key`: who signed with it, never their user id. */
export interface PluginKeyLookup {
  readonly key: string;
  /** The owner's current username; null if they have none. */
  readonly username: string | null;
  /** Whether it's the viewer's own key. */
  readonly mine: boolean;
  readonly revoked: boolean;
  /** Not revoked, and its owner holds a live username: the only keys an
   * engine may show as "Made by @username". Anything else reads as Unknown. */
  readonly verified: boolean;
  /** `pluginKeyFingerprint`: what a person compares. */
  readonly fingerprint: string;
  /** When it was first registered, epoch ms. */
  readonly firstSeen: number;
  readonly revokedAt: number | null;
}

/** One of the caller's keys in `GET /plugin-keys`. Times are epoch ms. */
export interface PluginKeyRow {
  readonly key: string;
  readonly fingerprint: string;
  readonly deviceId: string | null;
  readonly createdAt: number;
  readonly revokedAt: number | null;
}

/** A key's fingerprint: the first 16 bytes of SHA-256 over the raw key,
 * lowercase hex in groups of four (`3f9a 0c12 …`). */
export const pluginKeyFingerprint = async (key: string): Promise<string> => {
  const raw = Uint8Array.from(atob(key.replaceAll("-", "+").replaceAll("_", "/")), (c) => c.charCodeAt(0));
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", raw)).subarray(0, 16);
  const hex = [...digest].map((b) => b.toString(16).padStart(2, "0")).join("");
  return hex.match(/.{4}/g)!.join(" ");
};

/** `reserveImage`'s answer. */
export type ImageReservation = "ok" | "too_many" | "too_large" | "rate_limited" | "deleted_user";

const fullName = (first: string | null, last: string | null): string | null =>
  [first, last].filter(Boolean).join(" ") || null;

export class UserDirectory extends DurableObject<Env> {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS users (
      user_id TEXT PRIMARY KEY, email TEXT, first_name TEXT, last_name TEXT,
      profile_picture_url TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
      deleted_at TEXT, recorded_at INTEGER NOT NULL, user_number INTEGER)`);
    const columns = [...this.ctx.storage.sql.exec("SELECT name FROM pragma_table_info('users')")].map((row) => row.name);
    if (!columns.includes("user_number")) this.ctx.storage.sql.exec("ALTER TABLE users ADD COLUMN user_number INTEGER");
    this.ctx.storage.sql.exec("CREATE UNIQUE INDEX IF NOT EXISTS users_by_number ON users (user_number)");
    this.numberUsers();
    // What they do, and their own photo (Settings → Profile): the edge's,
    // not WorkOS's, which keeps neither.
    if (!columns.includes("title")) this.ctx.storage.sql.exec("ALTER TABLE users ADD COLUMN title TEXT");
    if (!columns.includes("photo_key")) this.ctx.storage.sql.exec("ALTER TABLE users ADD COLUMN photo_key TEXT");
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS badges (
      badge_id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, icon TEXT,
      created_at INTEGER NOT NULL)`);
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS user_badges (
      user_id TEXT NOT NULL, badge_id TEXT NOT NULL, granted_at INTEGER NOT NULL, note TEXT,
      PRIMARY KEY (user_id, badge_id))`);
    this.ctx.storage.sql.exec("CREATE INDEX IF NOT EXISTS user_badges_by_badge ON user_badges (badge_id)");
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS organizations (
      org_id TEXT PRIMARY KEY, name TEXT, external_id TEXT, domains TEXT NOT NULL DEFAULT '[]',
      created_at TEXT NOT NULL, updated_at TEXT NOT NULL, deleted_at TEXT, recorded_at INTEGER NOT NULL)`);
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS organization_memberships (
      membership_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, user_id TEXT NOT NULL, role TEXT, status TEXT NOT NULL,
      created_at TEXT NOT NULL, updated_at TEXT NOT NULL, deleted_at TEXT, recorded_at INTEGER NOT NULL)`);
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS org_creations (
      user_id TEXT PRIMARY KEY, claim_id TEXT NOT NULL, claimed_at INTEGER NOT NULL, org_id TEXT, created_at INTEGER)`);
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS invite_quota (
      user_id TEXT PRIMARY KEY, window_start INTEGER NOT NULL, sent INTEGER NOT NULL)`);
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS onboarding_done (
      user_id TEXT NOT NULL, page_id TEXT NOT NULL, done_at INTEGER NOT NULL,
      PRIMARY KEY (user_id, page_id))`);
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS plugin_keys (
      key TEXT PRIMARY KEY, user_id TEXT NOT NULL, device_id TEXT,
      created_at INTEGER NOT NULL, revoked_at INTEGER)`);
    this.ctx.storage.sql.exec("CREATE INDEX IF NOT EXISTS plugin_keys_user ON plugin_keys(user_id)");
    // step-up.ts: one row per live sign-in ticket, by its hash.
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS step_ups (
      token_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL)`);
    this.ctx.storage.sql.exec("CREATE INDEX IF NOT EXISTS step_ups_user ON step_ups(user_id)");
    // Images put through `PUT /auth/me/images`: their owner's quota, and what
    // goes when the owner does. Deleted rows stay a day for the upload count.
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS user_images (
      image_key TEXT PRIMARY KEY, user_id TEXT NOT NULL, bytes INTEGER NOT NULL,
      created_at INTEGER NOT NULL, deleted_at INTEGER)`);
    this.ctx.storage.sql.exec("CREATE INDEX IF NOT EXISTS user_images_user ON user_images(user_id)");
    // A role change or removal in an org at a time (`lockOrg`).
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS org_locks (
      org_id TEXT PRIMARY KEY, holder TEXT NOT NULL, expires_at INTEGER NOT NULL)`);
    this.ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS invite_quota_day (
      user_id TEXT PRIMARY KEY, window_start INTEGER NOT NULL, sent INTEGER NOT NULL)`);
  }

  /** Number every user still without one — the rows recorded before numbers
   * existed — in join order, after the highest number held. */
  private numberUsers(): void {
    this.ctx.storage.transactionSync(() => {
      let next = (this.ctx.storage.sql.exec("SELECT COALESCE(MAX(user_number), 0) AS n FROM users").one().n as number) + 1;
      const unnumbered = [
        ...this.ctx.storage.sql.exec("SELECT user_id FROM users WHERE user_number IS NULL ORDER BY created_at, user_id")
      ];
      for (const row of unnumbered) {
        this.ctx.storage.sql.exec("UPDATE users SET user_number = ? WHERE user_id = ?", next++, row.user_id);
      }
    });
  }

  /** Define a badge, or rename/redescribe one (its id and creation time stay). */
  defineBadge(badge: { readonly id: string; readonly name: string; readonly description: string | null; readonly icon: string | null }): Badge {
    this.ctx.storage.sql.exec(
      `INSERT INTO badges (badge_id, name, description, icon, created_at) VALUES (?, ?, ?, ?, ?)
       ON CONFLICT (badge_id) DO UPDATE SET
         name = excluded.name, description = excluded.description, icon = excluded.icon`,
      badge.id,
      badge.name,
      badge.description,
      badge.icon,
      Date.now()
    );
    return this.listBadges().find((b) => b.id === badge.id)!;
  }

  listBadges(): Badge[] {
    return [
      ...this.ctx.storage.sql.exec(
        `SELECT b.*, (SELECT COUNT(*) FROM user_badges ub WHERE ub.badge_id = b.badge_id) AS holders
         FROM badges b ORDER BY b.created_at`
      )
    ].map((row) => ({
      id: row.badge_id as string,
      name: row.name as string,
      description: row.description as string | null,
      icon: row.icon as string | null,
      createdAt: row.created_at as number,
      holders: row.holders as number
    }));
  }

  /** Grant `badgeId` to each user (`"all"` = every live directory user).
   * Re-granting keeps the first grant. Undefined when the badge isn't defined;
   * otherwise how many users newly hold it. */
  grantBadge(badgeId: string, userIds: readonly string[] | "all", note: string | null): number | undefined {
    if (!this.badgeExists(badgeId)) return undefined;
    const now = Date.now();
    if (userIds === "all") {
      // Counted by RETURNING rows: `rowsWritten` also counts index writes.
      return [
        ...this.ctx.storage.sql.exec(
          `INSERT OR IGNORE INTO user_badges (user_id, badge_id, granted_at, note)
           SELECT user_id, ?, ?, ? FROM users WHERE deleted_at IS NULL RETURNING user_id`,
          badgeId,
          now,
          note
        )
      ].length;
    }
    let granted = 0;
    this.ctx.storage.transactionSync(() => {
      for (const userId of userIds) {
        granted += [
          ...this.ctx.storage.sql.exec(
            "INSERT OR IGNORE INTO user_badges (user_id, badge_id, granted_at, note) VALUES (?, ?, ?, ?) RETURNING user_id",
            userId,
            badgeId,
            now,
            note
          )
        ].length;
      }
    });
    return granted;
  }

  /** Take `badgeId` back from each user. Answers how many held it. */
  revokeBadge(badgeId: string, userIds: readonly string[]): number {
    let revoked = 0;
    this.ctx.storage.transactionSync(() => {
      for (const userId of userIds) {
        revoked += [
          ...this.ctx.storage.sql.exec(
            "DELETE FROM user_badges WHERE user_id = ? AND badge_id = ? RETURNING user_id",
            userId,
            badgeId
          )
        ].length;
      }
    });
    return revoked;
  }

  private badgeExists(badgeId: string): boolean {
    return [...this.ctx.storage.sql.exec("SELECT 1 FROM badges WHERE badge_id = ?", badgeId)].length > 0;
  }

  /** Record a user's current profile; stale or post-delete writes are no-ops.
   * A new user takes the next user number. Answers whether the row changed. */
  upsert(user: WorkOsUser): boolean {
    const cursor = this.ctx.storage.sql.exec(
      `INSERT INTO users (user_id, email, first_name, last_name, profile_picture_url, created_at, updated_at, recorded_at, user_number)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ${NEXT_USER_NUMBER})
       ON CONFLICT (user_id) DO UPDATE SET
         email = excluded.email, first_name = excluded.first_name, last_name = excluded.last_name,
         profile_picture_url = excluded.profile_picture_url, created_at = excluded.created_at,
         updated_at = excluded.updated_at, recorded_at = excluded.recorded_at
       WHERE users.deleted_at IS NULL AND excluded.updated_at >= users.updated_at`,
      user.id,
      user.email,
      user.first_name,
      user.last_name,
      user.profile_picture_url,
      user.created_at,
      user.updated_at,
      Date.now()
    );
    return cursor.rowsWritten > 0;
  }

  /** Tombstone a deleted user: keep id, number and joined date, drop the
   * profile, badges and onboarding. */
  remove(user: WorkOsUser, deletedAt: string): void {
    this.ctx.storage.sql.exec("DELETE FROM user_badges WHERE user_id = ?", user.id);
    this.ctx.storage.sql.exec("DELETE FROM onboarding_done WHERE user_id = ?", user.id);
    // Their plugins stop reading as theirs (or anyone's).
    this.ctx.storage.sql.exec("UPDATE plugin_keys SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL", Date.now(), user.id);
    this.ctx.storage.sql.exec("DELETE FROM step_ups WHERE user_id = ?", user.id);
    this.ctx.storage.sql.exec(
      `INSERT INTO users (user_id, created_at, updated_at, deleted_at, recorded_at, user_number)
       VALUES (?, ?, ?, ?, ?, ${NEXT_USER_NUMBER})
       ON CONFLICT (user_id) DO UPDATE SET
         email = NULL, first_name = NULL, last_name = NULL, profile_picture_url = NULL,
         title = NULL, photo_key = NULL,
         deleted_at = excluded.deleted_at, recorded_at = excluded.recorded_at`,
      user.id,
      user.created_at,
      user.updated_at,
      deletedAt,
      Date.now()
    );
  }

  /** The user's own photo's key, or null (deleted users have none). */
  photoKey(userId: string): string | null {
    const [row] = this.ctx.storage.sql.exec("SELECT photo_key FROM users WHERE user_id = ?", userId).toArray();
    return (row?.photo_key as string | null | undefined) ?? null;
  }

  /** The live profiles of those of `userIds` the
   * directory has: an org's member list joins on this before asking WorkOS
   * for the rest. Keyed by user id; deleted users are left out. */
  profiles(userIds: readonly string[]): Record<string, MemberProfile> {
    const found: Record<string, MemberProfile> = {};
    for (const userId of new Set(userIds)) {
      const [row] = this.ctx.storage.sql
        .exec(
          `SELECT u.email, u.first_name, u.last_name, u.profile_picture_url, u.title, u.photo_key FROM users u
           WHERE u.user_id = ? AND u.deleted_at IS NULL`,
          userId
        )
        .toArray();
      if (!row) continue;
      found[userId] = {
        email: row.email as string | null,
        firstName: row.first_name as string | null,
        lastName: row.last_name as string | null,
        profilePictureUrl: row.profile_picture_url as string | null,
        username: null,
        title: row.title as string | null,
        photoKey: row.photo_key as string | null
      };
    }
    return found;
  }

  /** Set a live user's title (null clears it). False when the directory
   * doesn't have them live (not recorded yet, or deleted). */
  setTitle(userId: string, title: string | null): boolean {
    return (
      [...this.ctx.storage.sql.exec("UPDATE users SET title = ? WHERE user_id = ? AND deleted_at IS NULL RETURNING user_id", title, userId)]
        .length > 0
    );
  }

  /** Set a live user's photo key (null clears it). Answers the key it
   * replaced, for the caller to delete; undefined when the directory doesn't
   * have them live. */
  setPhoto(userId: string, key: string | null): { readonly replaced: string | null } | undefined {
    let answer: { readonly replaced: string | null } | undefined;
    this.ctx.storage.transactionSync(() => {
      const [row] = this.ctx.storage.sql.exec("SELECT photo_key FROM users WHERE user_id = ? AND deleted_at IS NULL", userId).toArray();
      if (!row) return;
      this.ctx.storage.sql.exec("UPDATE users SET photo_key = ? WHERE user_id = ?", key, userId);
      answer = { replaced: (row.photo_key as string | null) ?? null };
    });
    return answer;
  }

  /** Whether this account is unknown or deleted. */
  private userRefusal(userId: string): "unknown_user" | "deleted_user" | undefined {
    const [row] = this.ctx.storage.sql.exec("SELECT deleted_at FROM users WHERE user_id = ?", userId).toArray();
    if (!row) return "unknown_user";
    return row.deleted_at === null ? undefined : "deleted_user";
  }

  /** Take the user's one org creation before calling WorkOS. */
  claimOrgCreation(userId: string): OrgCreationClaim {
    return this.ctx.storage.transactionSync((): OrgCreationClaim => {
      const now = Date.now();
      const [row] = this.ctx.storage.sql.exec("SELECT org_id, claimed_at FROM org_creations WHERE user_id = ?", userId).toArray();
      if (row && (row.org_id !== null || now - (row.claimed_at as number) < ORG_CLAIM_STALE_MS)) {
        return { organizationId: row.org_id as string | null };
      }
      const claimId = crypto.randomUUID();
      this.ctx.storage.sql.exec(
        `INSERT INTO org_creations (user_id, claim_id, claimed_at) VALUES (?, ?, ?)
         ON CONFLICT (user_id) DO UPDATE SET claim_id = excluded.claim_id, claimed_at = excluded.claimed_at`,
        userId,
        claimId,
        now
      );
      return { claimId };
    });
  }

  /** Record the org a claim created. */
  finishOrgCreation(userId: string, claimId: string, orgId: string): void {
    this.ctx.storage.sql.exec(
      "UPDATE org_creations SET org_id = ?, created_at = ? WHERE user_id = ? AND claim_id = ? AND org_id IS NULL",
      orgId,
      Date.now(),
      userId,
      claimId
    );
  }

  /** Give a claim back when its create didn't happen. */
  releaseOrgCreation(userId: string, claimId: string): void {
    this.ctx.storage.sql.exec("DELETE FROM org_creations WHERE user_id = ? AND claim_id = ? AND org_id IS NULL", userId, claimId);
  }

  /** Count `count` invitation addresses against the user's hour and day;
   * false (and nothing counted) when that would pass `INVITES_PER_HOUR` or
   * `INVITES_PER_DAY`. */
  takeInviteQuota(userId: string, count: number): boolean {
    return this.ctx.storage.transactionSync(() => {
      const now = Date.now();
      const windows = [
        { table: "invite_quota", span: 3_600_000, cap: INVITES_PER_HOUR },
        { table: "invite_quota_day", span: 86_400_000, cap: INVITES_PER_DAY }
      ].map(({ table, span, cap }) => {
        const [row] = this.ctx.storage.sql.exec(`SELECT window_start, sent FROM ${table} WHERE user_id = ?`, userId).toArray();
        const fresh = !row || now - (row.window_start as number) >= span;
        return { table, cap, start: fresh ? now : (row.window_start as number), sent: fresh ? 0 : (row.sent as number) };
      });
      if (windows.some((w) => w.sent + count > w.cap)) return false;
      for (const w of windows) {
        this.ctx.storage.sql.exec(
          `INSERT INTO ${w.table} (user_id, window_start, sent) VALUES (?, ?, ?)
           ON CONFLICT (user_id) DO UPDATE SET window_start = excluded.window_start, sent = excluded.sent`,
          userId,
          w.start,
          w.sent + count
        );
      }
      return true;
    });
  }

  /** Mint a sign-in's step-up ticket (step-up.ts) for `userId`: store its
   * hash, keep at most `max` live per user (oldest go), drop expired ones. */
  issueStepUp(userId: string, tokenHash: string, ttlMs: number, max: number): number {
    const now = Date.now();
    const expiresAt = now + ttlMs;
    this.ctx.storage.transactionSync(() => {
      this.ctx.storage.sql.exec("DELETE FROM step_ups WHERE expires_at <= ?", now);
      this.ctx.storage.sql.exec(
        "INSERT INTO step_ups (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
        tokenHash,
        userId,
        now,
        expiresAt
      );
      this.ctx.storage.sql.exec(
        `DELETE FROM step_ups WHERE user_id = ? AND token_hash NOT IN
           (SELECT token_hash FROM step_ups WHERE user_id = ? ORDER BY created_at DESC, token_hash LIMIT ?)`,
        userId,
        userId,
        max
      );
    });
    return expiresAt;
  }

  /** Whether `tokenHash` is a live ticket of `userId`'s (nothing used up). */
  hasStepUp(userId: string, tokenHash: string | undefined): boolean {
    if (!tokenHash) return false;
    return (
      this.ctx.storage.sql
        .exec("SELECT 1 FROM step_ups WHERE token_hash = ? AND user_id = ? AND expires_at > ?", tokenHash, userId, Date.now())
        .toArray().length > 0
    );
  }

  /** Use up a live ticket of `userId`'s; false when it isn't one. */
  useStepUp(userId: string, tokenHash: string | undefined): boolean {
    if (!tokenHash) return false;
    return (
      this.ctx.storage.sql
        .exec(
          "DELETE FROM step_ups WHERE token_hash = ? AND user_id = ? AND expires_at > ? RETURNING token_hash",
          tokenHash,
          userId,
          Date.now()
        )
        .toArray().length > 0
    );
  }

  /** Count an image of `bytes` against its owner's quota before it's stored. */
  reserveImage(userId: string, imageKey: string, bytes: number): ImageReservation {
    return this.ctx.storage.transactionSync((): ImageReservation => {
      if (this.userRefusal(userId) === "deleted_user") return "deleted_user";
      const now = Date.now();
      this.ctx.storage.sql.exec("DELETE FROM user_images WHERE deleted_at IS NOT NULL AND deleted_at <= ?", now - 86_400_000);
      const held = this.ctx.storage.sql
        .exec("SELECT COUNT(*) AS n, COALESCE(SUM(bytes), 0) AS b FROM user_images WHERE user_id = ? AND deleted_at IS NULL", userId)
        .one();
      const today = this.ctx.storage.sql
        .exec("SELECT COUNT(*) AS n FROM user_images WHERE user_id = ? AND created_at > ?", userId, now - 86_400_000)
        .one().n as number;
      if (today >= IMAGE_UPLOADS_PER_DAY) return "rate_limited";
      if ((held.n as number) >= MAX_IMAGES_PER_USER) return "too_many";
      if ((held.b as number) + bytes > MAX_IMAGE_BYTES_PER_USER) return "too_large";
      this.ctx.storage.sql.exec(
        "INSERT INTO user_images (image_key, user_id, bytes, created_at) VALUES (?, ?, ?, ?)",
        imageKey,
        userId,
        bytes,
        now
      );
      return "ok";
    });
  }

  /** An image of `userId`'s is gone (deleted, or never stored). */
  dropImage(userId: string, imageKey: string): void {
    this.ctx.storage.sql.exec(
      "UPDATE user_images SET deleted_at = ? WHERE image_key = ? AND user_id = ? AND deleted_at IS NULL",
      Date.now(),
      imageKey,
      userId
    );
  }

  /** Every live image of `userId`'s, marked gone: for the caller to delete
   * with the account. */
  takeImages(userId: string): string[] {
    return this.ctx.storage.sql
      .exec("UPDATE user_images SET deleted_at = ? WHERE user_id = ? AND deleted_at IS NULL RETURNING image_key", Date.now(), userId)
      .toArray()
      .map((row) => row.image_key as string);
  }

  /** Take `orgId`'s role lock for `holder`, so only one role change, removal
   * or admin's deletion runs in it at a time. False while someone else holds
   * it; a lock older than `ORG_LOCK_MS` lapses. */
  lockOrg(orgId: string, holder: string): boolean {
    const now = Date.now();
    return (
      this.ctx.storage.sql
        .exec(
          `INSERT INTO org_locks (org_id, holder, expires_at) VALUES (?, ?, ?)
           ON CONFLICT (org_id) DO UPDATE SET holder = excluded.holder, expires_at = excluded.expires_at
           WHERE org_locks.expires_at <= ? OR org_locks.holder = excluded.holder
           RETURNING org_id`,
          orgId,
          holder,
          now + ORG_LOCK_MS,
          now
        )
        .toArray().length > 0
    );
  }

  unlockOrg(orgId: string, holder: string): void {
    this.ctx.storage.sql.exec("DELETE FROM org_locks WHERE org_id = ? AND holder = ?", orgId, holder);
  }

  /** The onboarding pages a user finished: page id → ISO time first done. */
  onboarding(userId: string): Record<string, string> {
    const done: Record<string, string> = {};
    for (const row of this.ctx.storage.sql.exec(
      "SELECT page_id, done_at FROM onboarding_done WHERE user_id = ? ORDER BY done_at, page_id",
      userId
    )) {
      done[row.page_id as string] = new Date(row.done_at as number).toISOString();
    }
    return done;
  }

  /** Record pages as finished, keeping the first time for any already done.
   * Undefined (and nothing recorded) past `MAX_ONBOARDING_PAGES`. */
  markOnboarding(userId: string, pageIds: readonly string[]): Record<string, string> | undefined {
    const recorded = this.ctx.storage.transactionSync(() => {
      const held = new Set(
        this.ctx.storage.sql.exec("SELECT page_id FROM onboarding_done WHERE user_id = ?", userId).toArray().map((r) => r.page_id as string)
      );
      const fresh = [...new Set(pageIds)].filter((id) => !held.has(id));
      if (held.size + fresh.length > MAX_ONBOARDING_PAGES) return false;
      const now = Date.now();
      for (const pageId of fresh) {
        this.ctx.storage.sql.exec("INSERT INTO onboarding_done (user_id, page_id, done_at) VALUES (?, ?, ?)", userId, pageId, now);
      }
      return true;
    });
    return recorded ? this.onboarding(userId) : undefined;
  }

  /** Bind a plugin signing key (proof already checked) to `userId`. A new
   * key needs a sign-in's step-up ticket (`stepUpHash`, step-up.ts), used up
   * here. The same key again from its owner is a no-op that succeeds without
   * one, so an engine can retry a registration whose answer it lost. A
   * revoked key stays revoked. */
  registerPluginKey(userId: string, key: string, deviceId: string | null, stepUpHash?: string): PluginKeyRegistration {
    return this.ctx.storage.transactionSync((): PluginKeyRegistration => {
      const [row] = this.ctx.storage.sql.exec("SELECT user_id, revoked_at FROM plugin_keys WHERE key = ?", key).toArray();
      if (row) {
        if (row.user_id !== userId) return "key_taken";
        return row.revoked_at === null ? "registered" : "key_revoked";
      }
      if (this.userRefusal(userId) === "deleted_user") return "deleted_user";
      const now = Date.now();
      const today = this.ctx.storage.sql
        .exec("SELECT COUNT(*) AS n FROM plugin_keys WHERE user_id = ? AND created_at > ?", userId, now - 86_400_000)
        .one().n as number;
      if (today >= PLUGIN_KEYS_PER_DAY) return "rate_limited";
      if (!this.useStepUp(userId, stepUpHash)) return "step_up_required";
      this.ctx.storage.sql.exec(
        "INSERT INTO plugin_keys (key, user_id, device_id, created_at) VALUES (?, ?, ?, ?)",
        key,
        userId,
        deviceId,
        now
      );
      return "registered";
    });
  }

  /** Who a plugin key belongs to, as `viewerId` may see it: never the owner's
   * user id. Undefined for a key never registered, and, to anyone but its
   * owner, for a live key whose owner has no username: nothing vouches for
   * it, so it reads as unknown to every engine, old ones included. Read
   * straight from storage on every call, so a revocation shows at once. */
  async pluginKey(key: string, viewerId: string): Promise<PluginKeyLookup | undefined> {
    const [row] = this.ctx.storage.sql.exec("SELECT user_id, created_at, revoked_at FROM plugin_keys WHERE key = ?", key).toArray();
    if (!row) return undefined;
    const username = null;
    const mine = row.user_id === viewerId;
    const revoked = row.revoked_at !== null;
    if (!mine && !revoked && username === null) return undefined;
    return {
      key,
      username,
      mine,
      revoked,
      verified: !revoked && username !== null,
      fingerprint: await pluginKeyFingerprint(key),
      firstSeen: row.created_at as number,
      revokedAt: row.revoked_at as number | null
    };
  }

  /** The device a live key of `userId`'s was registered from (device-link.ts),
   * or null: not theirs, revoked, or registered without one. */
  pluginKeyDevice(key: string, userId: string): string | null {
    const [row] = this.ctx.storage.sql
      .exec("SELECT device_id FROM plugin_keys WHERE key = ? AND user_id = ? AND revoked_at IS NULL", key, userId)
      .toArray();
    return (row?.device_id as string | null | undefined) ?? null;
  }

  /** A user's plugin keys, oldest first, revoked ones included. With `since`
   * (epoch ms), only those registered or revoked after it. */
  async pluginKeys(userId: string, since?: number): Promise<PluginKeyRow[]> {
    const rows = this.ctx.storage.sql
      .exec(
        `SELECT key, device_id, created_at, revoked_at FROM plugin_keys WHERE user_id = ?
           AND (? IS NULL OR created_at > ? OR revoked_at > ?) ORDER BY created_at, key`,
        userId,
        since ?? null,
        since ?? null,
        since ?? null
      )
      .toArray();
    return Promise.all(
      rows.map(async (row) => ({
        key: row.key as string,
        fingerprint: await pluginKeyFingerprint(row.key as string),
        deviceId: row.device_id as string | null,
        createdAt: row.created_at as number,
        revokedAt: row.revoked_at as number | null
      }))
    );
  }

  /** Revoke one of the user's keys (the first revocation's time stays).
   * False when the key isn't theirs. */
  revokePluginKey(userId: string, key: string): boolean {
    const [row] = this.ctx.storage.sql.exec("SELECT user_id FROM plugin_keys WHERE key = ?", key).toArray();
    if (row?.user_id !== userId) return false;
    this.ctx.storage.sql.exec("UPDATE plugin_keys SET revoked_at = COALESCE(revoked_at, ?) WHERE key = ?", Date.now(), key);
    return true;
  }

  list(): DirectoryUser[] {
    const onboarding = new Map<string, string[]>();
    for (const row of this.ctx.storage.sql.exec("SELECT user_id, page_id FROM onboarding_done ORDER BY done_at, page_id")) {
      const done = onboarding.get(row.user_id as string) ?? [];
      done.push(row.page_id as string);
      onboarding.set(row.user_id as string, done);
    }
    const badges = new Map<string, UserBadge[]>();
    for (const row of this.ctx.storage.sql.exec("SELECT * FROM user_badges ORDER BY granted_at, badge_id")) {
      const userId = row.user_id as string;
      const held = badges.get(userId) ?? [];
      held.push({ id: row.badge_id as string, grantedAt: row.granted_at as number, note: row.note as string | null });
      badges.set(userId, held);
    }
    return [
      ...this.ctx.storage.sql.exec(
        `SELECT u.*, c.org_id AS created_org_id FROM users u
         LEFT JOIN org_creations c ON c.user_id = u.user_id
         ORDER BY u.created_at`
      )
    ].map((row) => {
      const first = row.first_name as string | null;
      const last = row.last_name as string | null;
      return {
        userId: row.user_id as string,
        userNumber: row.user_number as number,
        email: row.email as string | null,
        name: fullName(first, last),
        firstName: first,
        lastName: last,
        profilePictureUrl: row.profile_picture_url as string | null,
        joinedAt: row.created_at as string,
        updatedAt: row.updated_at as string,
        deletedAt: row.deleted_at as string | null,
        badges: badges.get(row.user_id as string) ?? [],
        onboardingDone: onboarding.get(row.user_id as string) ?? [],
        createdOrgId: row.created_org_id as string | null
      };
    });
  }

  /** Record an org's current details; stale or post-delete writes are no-ops.
   * Answers whether the row changed. */
  upsertOrganization(org: WorkOsOrganization): boolean {
    const cursor = this.ctx.storage.sql.exec(
      `INSERT INTO organizations (org_id, name, external_id, domains, created_at, updated_at, recorded_at)
       VALUES (?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (org_id) DO UPDATE SET
         name = excluded.name, external_id = excluded.external_id, domains = excluded.domains,
         created_at = excluded.created_at, updated_at = excluded.updated_at, recorded_at = excluded.recorded_at
       WHERE organizations.deleted_at IS NULL AND excluded.updated_at >= organizations.updated_at`,
      org.id,
      org.name,
      org.external_id,
      JSON.stringify(org.domains),
      org.created_at,
      org.updated_at,
      Date.now()
    );
    return cursor.rowsWritten > 0;
  }

  /** Tombstone a deleted org: keep id and creation date, drop the details
   * (an org is often named after its one person). */
  removeOrganization(org: WorkOsOrganization, deletedAt: string): void {
    this.ctx.storage.sql.exec(
      `INSERT INTO organizations (org_id, created_at, updated_at, deleted_at, recorded_at)
       VALUES (?, ?, ?, ?, ?)
       ON CONFLICT (org_id) DO UPDATE SET
         name = NULL, external_id = NULL, domains = '[]',
         deleted_at = excluded.deleted_at, recorded_at = excluded.recorded_at`,
      org.id,
      org.created_at,
      org.updated_at,
      deletedAt,
      Date.now()
    );
  }

  /** Record a membership's current role and status; stale or post-delete
   * writes are no-ops. Answers whether the row changed. */
  upsertMembership(membership: WorkOsMembership): boolean {
    const cursor = this.ctx.storage.sql.exec(
      `INSERT INTO organization_memberships (membership_id, org_id, user_id, role, status, created_at, updated_at, recorded_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (membership_id) DO UPDATE SET
         role = excluded.role, status = excluded.status, created_at = excluded.created_at,
         updated_at = excluded.updated_at, recorded_at = excluded.recorded_at
       WHERE organization_memberships.deleted_at IS NULL AND excluded.updated_at >= organization_memberships.updated_at`,
      membership.id,
      membership.organization_id,
      membership.user_id,
      membership.role,
      membership.status,
      membership.created_at,
      membership.updated_at,
      Date.now()
    );
    return cursor.rowsWritten > 0;
  }

  /** Tombstone a removed membership, which nothing revives. */
  removeMembership(membership: WorkOsMembership, deletedAt: string): void {
    this.ctx.storage.sql.exec(
      `INSERT INTO organization_memberships (membership_id, org_id, user_id, role, status, created_at, updated_at, deleted_at, recorded_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (membership_id) DO UPDATE SET
         deleted_at = excluded.deleted_at, recorded_at = excluded.recorded_at`,
      membership.id,
      membership.organization_id,
      membership.user_id,
      membership.role,
      membership.status,
      membership.created_at,
      membership.updated_at,
      deletedAt,
      Date.now()
    );
  }

  listOrganizations(): DirectoryOrganization[] {
    const members = new Map<string, OrganizationMember[]>();
    for (const row of this.ctx.storage.sql.exec(
      `SELECT m.*, u.user_number, u.email, u.first_name, u.last_name, u.profile_picture_url
       FROM organization_memberships m LEFT JOIN users u ON u.user_id = m.user_id
       WHERE m.deleted_at IS NULL AND u.deleted_at IS NULL
       ORDER BY m.created_at, m.membership_id`
    )) {
      const orgId = row.org_id as string;
      const held = members.get(orgId) ?? [];
      held.push({
        membershipId: row.membership_id as string,
        userId: row.user_id as string,
        userNumber: row.user_number as number | null,
        email: row.email as string | null,
        name: fullName(row.first_name as string | null, row.last_name as string | null),
        profilePictureUrl: row.profile_picture_url as string | null,
        role: row.role as string | null,
        status: row.status as string,
        joinedAt: row.created_at as string
      });
      members.set(orgId, held);
    }
    return [
      ...this.ctx.storage.sql.exec(
        `SELECT o.*, c.user_id AS created_by FROM organizations o
         LEFT JOIN org_creations c ON c.org_id = o.org_id
         ORDER BY o.created_at, o.org_id`
      )
    ].map((row) => {
      const held = row.deleted_at === null ? (members.get(row.org_id as string) ?? []) : [];
      return {
        orgId: row.org_id as string,
        name: row.name as string | null,
        externalId: row.external_id as string | null,
        domains: JSON.parse(row.domains as string) as string[],
        createdAt: row.created_at as string,
        updatedAt: row.updated_at as string,
        deletedAt: row.deleted_at as string | null,
        createdBy: row.created_by as string | null,
        memberCount: held.filter((m) => m.status === "active").length,
        members: held
      };
    });
  }
}

export const userDirectoryStub = (env: Pick<Env, "USER_DIRECTORY">) =>
  env.USER_DIRECTORY ? env.USER_DIRECTORY.get(env.USER_DIRECTORY.idFromName(USER_DIRECTORY_NAME)) : undefined;

// ── webhook signature ───────────────────────────────────────────────────────

/** How far a webhook's signed timestamp may drift (the WorkOS SDK default). */
export const WORKOS_SIGNATURE_TOLERANCE_MS = 180_000;

const hex = (bytes: ArrayBuffer): string =>
  [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");

const timingSafeEqual = (a: string, b: string): boolean => {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
};

/** Sign `payload` the way WorkOS does: hex HMAC-SHA256 of `{ms}.{body}`. */
export const signWorkOsPayload = async (secret: string, timestampMs: number, payload: string): Promise<string> => {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  return hex(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(`${timestampMs}.${payload}`)));
};

/** `WorkOS-Signature: t={ms}, v1={hex}` over the raw body, within tolerance. */
export const verifyWorkOsSignature = async (
  secret: string,
  header: string | null,
  payload: string,
  nowMs = Date.now()
): Promise<boolean> => {
  if (!header) return false;
  let timestamp: number | undefined;
  let signature: string | undefined;
  for (const part of header.split(",")) {
    const [key, value] = part.trim().split("=", 2);
    if (key === "t" && value) timestamp = Number(value);
    if (key === "v1" && value) signature = value;
  }
  if (!timestamp || !Number.isFinite(timestamp) || !signature) return false;
  if (Math.abs(nowMs - timestamp) > WORKOS_SIGNATURE_TOLERANCE_MS) return false;
  return timingSafeEqual(signature, await signWorkOsPayload(secret, timestamp, payload));
};

// ── routes ──────────────────────────────────────────────────────────────────

/** WorkOS events are a few KB; anything larger isn't one. */
const MAX_WEBHOOK_BODY = 256 * 1024;

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const optionalString = (value: unknown): string | null => (typeof value === "string" && value ? value : null);

/** The user in a `user.*` event's `data`, or undefined if it isn't one. */
const parseUser = (data: unknown): WorkOsUser | undefined => {
  if (!isObject(data)) return undefined;
  const { id, email, created_at, updated_at } = data;
  if (typeof id !== "string" || !id.startsWith("user_")) return undefined;
  if (typeof email !== "string" || typeof created_at !== "string" || typeof updated_at !== "string") return undefined;
  return {
    id,
    email,
    first_name: optionalString(data.first_name),
    last_name: optionalString(data.last_name),
    profile_picture_url: optionalString(data.profile_picture_url),
    created_at,
    updated_at
  };
};

/** The organization in an `organization.*` event's `data`, or undefined. */
const parseOrganization = (data: unknown): WorkOsOrganization | undefined => {
  if (!isObject(data)) return undefined;
  const { id, name, domains, created_at, updated_at } = data;
  if (typeof id !== "string" || !id.startsWith("org_")) return undefined;
  if (typeof name !== "string" || typeof created_at !== "string" || typeof updated_at !== "string") return undefined;
  return {
    id,
    name,
    external_id: optionalString(data.external_id),
    domains: Array.isArray(domains)
      ? domains.flatMap((d: unknown) => (isObject(d) && typeof d.domain === "string" ? [d.domain] : []))
      : [],
    created_at,
    updated_at
  };
};

/** The membership in an `organization_membership.*` event's `data`, or undefined. */
const parseMembership = (data: unknown): WorkOsMembership | undefined => {
  if (!isObject(data)) return undefined;
  const { id, organization_id, user_id, role, status, created_at, updated_at } = data;
  if (typeof id !== "string" || !id.startsWith("om_")) return undefined;
  if (typeof organization_id !== "string" || !organization_id.startsWith("org_")) return undefined;
  if (typeof user_id !== "string" || !user_id.startsWith("user_")) return undefined;
  if (typeof status !== "string" || typeof created_at !== "string" || typeof updated_at !== "string") return undefined;
  return {
    id,
    organization_id,
    user_id,
    role: isObject(role) ? optionalString(role.slug) : null,
    status,
    created_at,
    updated_at
  };
};

const csvField = (value: string | null): string =>
  value === null ? "" : /[",\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;

const csvResponse = (filename: string, header: string, rows: readonly (readonly (string | null)[])[]): Response =>
  new Response(`${[header, ...rows.map((row) => row.map(csvField).join(","))].join("\n")}\n`, {
    headers: {
      "content-type": "text/csv; charset=utf-8",
      "content-disposition": `attachment; filename="${filename}"`
    }
  });

const BADGE_ID_RE = /^[a-z0-9][a-z0-9-]{0,63}$/;
const USER_ID_RE = /^user_[A-Za-z0-9]{1,64}$/;
/** One grant/revoke call; `"all"` covers the whole directory in one statement. */
const MAX_BADGE_USERS = 5000;

const readJson = async (request: Request): Promise<Record<string, unknown> | undefined> => {
  try {
    const body: unknown = await request.json();
    return isObject(body) ? body : undefined;
  } catch {
    return undefined;
  }
};

/** A string of at most `max` chars, null when absent; false when invalid. */
const optionalText = (value: unknown, max: number): string | null | false =>
  value === undefined || value === null ? null : typeof value === "string" && value.length <= max ? value || null : false;

/**
 * `/admin/badges`:
 *   GET                                   — every badge with its holder count
 *   POST {id, name, description?, icon?}  — define (or rename) a badge
 *   POST /grant  {badgeId, userIds | "all", note?} — answers {granted}
 *   POST /revoke {badgeId, userIds}       — answers {revoked}
 */
const handleBadgeRoute = async (
  request: Request,
  directory: DurableObjectStub<UserDirectory>,
  action: string
): Promise<Response> => {
  if (action === "") {
    if (request.method === "GET") return json({ badges: await directory.listBadges() });
    if (request.method !== "POST") return jsonError(405, "method_not_allowed", "GET or POST only.");
    const body = await readJson(request);
    const description = optionalText(body?.description, 500);
    const icon = optionalText(body?.icon, 64);
    if (
      !body ||
      typeof body.id !== "string" ||
      !BADGE_ID_RE.test(body.id) ||
      typeof body.name !== "string" ||
      !body.name.trim() ||
      body.name.length > 80 ||
      description === false ||
      icon === false
    ) {
      return jsonError(400, "bad_request", "Expected {id (a-z, 0-9, -), name, description?, icon?}.");
    }
    return json(await directory.defineBadge({ id: body.id, name: body.name.trim(), description, icon }));
  }
  if (action !== "grant" && action !== "revoke") return jsonError(404, "not_found", "Not found.");
  if (request.method !== "POST") return jsonError(405, "method_not_allowed", "POST only.");
  const body = await readJson(request);
  const note = optionalText(body?.note, 500);
  const userIds = body?.userIds;
  const validIds =
    Array.isArray(userIds) &&
    userIds.length > 0 &&
    userIds.length <= MAX_BADGE_USERS &&
    userIds.every((id) => typeof id === "string" && USER_ID_RE.test(id));
  if (
    !body ||
    typeof body.badgeId !== "string" ||
    !BADGE_ID_RE.test(body.badgeId) ||
    note === false ||
    !(validIds || (action === "grant" && userIds === "all"))
  ) {
    return jsonError(
      400,
      "bad_request",
      `Expected {badgeId, userIds (1–${MAX_BADGE_USERS} user ids${action === "grant" ? ' or "all"' : ""})${action === "grant" ? ", note?" : ""}}.`
    );
  }
  const ids = userIds === "all" ? "all" : [...new Set(userIds as string[])];
  if (action === "revoke") return json({ revoked: await directory.revokeBadge(body.badgeId, ids as string[]) });
  const granted = await directory.grantBadge(body.badgeId, ids, note);
  if (granted === undefined) return jsonError(404, "unknown_badge", "Define the badge first (POST /admin/badges).");
  return json({ granted });
};

/**
 * `POST /workos/webhook` (WorkOS-signed, no bearer), `GET /admin/users`,
 * `GET /admin/organizations` and `/admin/badges[/grant|/revoke]` (ADMIN_TOKEN)
 * — routed BEFORE the bearer gate. Events other than the created, updated and
 * deleted of users, organizations and memberships are acknowledged so WorkOS
 * stops retrying.
 */
export const handleUserDirectoryRoute = async (
  request: Request,
  env: Env,
  url: URL
): Promise<Response | undefined> => {
  const badgeAction =
    url.pathname === "/admin/badges" ? "" : url.pathname.startsWith("/admin/badges/") ? url.pathname.slice(14) : undefined;
  if (url.pathname === "/admin/users" || url.pathname === "/admin/organizations" || badgeAction !== undefined) {
    if (!env.ADMIN_TOKEN) return json({ error: "not_found" }, 404);
    if (!(await adminAuthorized(request, env.ADMIN_TOKEN))) {
      return jsonError(401, "unauthenticated", "Admin token required.");
    }
    const directory = userDirectoryStub(env);
    if (!directory) return jsonError(503, "unavailable", "The user directory isn't set up on this server.");
    if (badgeAction !== undefined) return handleBadgeRoute(request, directory, badgeAction);
    if (request.method !== "GET") return jsonError(405, "method_not_allowed", "GET only.");
    const csv = url.searchParams.get("format") === "csv";
    if (url.pathname === "/admin/organizations") {
      const organizations = await directory.listOrganizations();
      if (!csv) return json({ count: organizations.length, organizations });
      return csvResponse(
        "zerun-organizations.csv",
        "org_id,name,external_id,domains,member_count,members,created_at,updated_at,deleted_at,created_by",
        organizations.map((o) => [
          o.orgId,
          o.name,
          o.externalId,
          o.domains.join(";") || null,
          String(o.memberCount),
          o.members
            .filter((m) => m.status === "active")
            .map((m) => m.email ?? m.userId)
            .join(";") || null,
          o.createdAt,
          o.updatedAt,
          o.deletedAt,
          o.createdBy
        ])
      );
    }
    const users = await directory.list();
    if (!csv) return json({ count: users.length, users });
    return csvResponse(
      "zerun-users.csv",
      "user_number,user_id,email,name,first_name,last_name,profile_picture_url,joined_at,updated_at,deleted_at,badges,created_org_id,onboarding_done",
      users.map((u) => [
        String(u.userNumber),
        u.userId,
        u.email,
        u.name,
        u.firstName,
        u.lastName,
        u.profilePictureUrl,
        u.joinedAt,
        u.updatedAt,
        u.deletedAt,
        u.badges.map((b) => b.id).join(";") || null,
        u.createdOrgId,
        u.onboardingDone.join(";") || null
      ])
    );
  }

  if (url.pathname !== "/workos/webhook") return undefined;
  if (request.method !== "POST") return jsonError(405, "method_not_allowed", "POST only.");
  const directory = userDirectoryStub(env);
  if (!env.WORKOS_WEBHOOK_SECRET || !directory) {
    return jsonError(503, "unavailable", "WorkOS webhooks aren't set up on this server.");
  }
  const payload = await request.text();
  if (payload.length > MAX_WEBHOOK_BODY) return jsonError(413, "too_large", "Body too large.");
  if (!(await verifyWorkOsSignature(env.WORKOS_WEBHOOK_SECRET, request.headers.get("workos-signature"), payload))) {
    return jsonError(400, "bad_signature", "Signature check failed.");
  }
  let event: unknown;
  try {
    event = JSON.parse(payload);
  } catch {
    return jsonError(400, "bad_request", "Body must be JSON.");
  }
  if (!isObject(event) || typeof event.event !== "string") {
    return jsonError(400, "bad_request", "Body must be a WorkOS event.");
  }
  const eventId = event.id;
  const deletedAt = typeof event.created_at === "string" ? event.created_at : new Date().toISOString();
  /** A delete answers nothing and always lands; an upsert answers whether it did. */
  const apply = async (write: () => Promise<boolean | void>): Promise<Response> => {
    try {
      return json({ received: true, applied: (await write()) ?? true });
    } catch (e) {
      console.error("users: applying a webhook failed", eventId, String(e));
      // WorkOS retries a non-2xx: nothing is lost.
      return jsonError(503, "unavailable", "Try again.");
    }
  };
  switch (event.event) {
    case "user.created":
    case "user.updated":
    case "user.deleted": {
      const user = parseUser(event.data);
      if (!user) return jsonError(400, "bad_request", "Event data must be a WorkOS user.");
      if (event.event !== "user.deleted") return apply(() => directory.upsert(user));
      return apply(async () => {
        // (Their own photo and images go too: auth-routes.ts keeps them at
        // avatars/{key}.)
        const photoKey = await directory.photoKey(user.id);
        const images = await directory.takeImages(user.id);
        await directory.remove(user, deletedAt);
        await deleteUserBlobs(env, user.id, photoKey, images);
      });
    }
    case "organization.created":
    case "organization.updated":
    case "organization.deleted": {
      const org = parseOrganization(event.data);
      if (!org) return jsonError(400, "bad_request", "Event data must be a WorkOS organization.");
      return apply(() =>
        event.event === "organization.deleted" ? directory.removeOrganization(org, deletedAt) : directory.upsertOrganization(org)
      );
    }
    case "organization_membership.created":
    case "organization_membership.updated":
    case "organization_membership.deleted": {
      const membership = parseMembership(event.data);
      if (!membership) return jsonError(400, "bad_request", "Event data must be a WorkOS organization membership.");
      return apply(() =>
        event.event === "organization_membership.deleted"
          ? directory.removeMembership(membership, deletedAt)
          : directory.upsertMembership(membership)
      );
    }
    default:
      return json({ received: true });
  }
};
