import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { creditRate, isDayKey, NO_CUTOVER } from "../../src/cloud/metering";
import { CREDIT_PACK, PLANS } from "../../src/cloud/stripe";

/** What the sandbox provider bills: billable seconds per dollar. */
const BILLABLE_SECONDS_PER_DOLLAR = 100_000;

const wrangler = readFileSync(new URL("../../wrangler.jsonc", import.meta.url), "utf8");
// The fork does not deploy Cloud pricing. Exercise the optional pricing
// policy with a representative configured rate, without enabling it here.
const rates = ["30"];
const cutovers = ["SET-AT-DEPLOY"];

const dollars = (credits: number, secondsPerCredit: number) =>
  (credits * secondsPerCredit) / BILLABLE_SECONDS_PER_DOLLAR;

describe("credit pricing", () => {
  it("leaves Cloud billing unconfigured in the fork", () => {
    expect(wrangler).not.toMatch(/^\s*"CLOUD_CREDIT_SECONDS":/m);
    expect(wrangler).not.toMatch(/^\s*"CLOUD_CREDIT_RATE_FROM":/m);
  });

  it.each(PLANS.map((plan) => [plan.name, plan] as const))("%s costs less at full use than its price", (_name, plan) => {
    for (const seconds of rates) {
      expect(dollars(plan.monthlyCredits, Number(seconds))).toBeLessThan(plan.priceCents / 100);
    }
  });

  it("a credit pack costs less than its price", () => {
    for (const seconds of rates) {
      expect(dollars(CREDIT_PACK.credits, Number(seconds))).toBeLessThan(CREDIT_PACK.priceCents / 100);
    }
  });

  it("has a real day or the placeholder for a cutover, and the placeholder changes nothing", () => {
    for (const from of cutovers) {
      if (!isDayKey(from)) {
        expect(from).toBe("SET-AT-DEPLOY");
        expect(creditRate(rates[0], from)).toEqual(NO_CUTOVER);
      }
    }
  });
});
