--!strict
-- A lane in a chat's panel (screens/panel): a side chat, or a subagent, as
-- the shipping app's right-pane tabs show them (RightSurface::SideChat,
-- ::Subagent). The host watches its chat beside the open one, in a slot of
-- its own (the tab's id: `chat.open_lane`, let go as the tab goes). A side
-- chat is its transcript and a composer of its own (its draft its own, not
-- the chat page's); a subagent's is its transcript alone, to read.
--
--   LaneTab { tab = { id, kind = "side_chat" | "subagent", chat, title } }

local ds = require("@ds")
local Dictation = require("@app/screens/chat/dictation")
local T = require("@ds/transcript")
local lanes = require("@app/screens/chat/lanes")
local TurnGlue = require("@app/screens/chat/turn_glue")
local store = require("@app/screens/chat/store")
local prefs = require("@app/screens/settings/prefs")
local locale = require("@ds/locale")
local sessions = require("@account/sessions")
local route = require("@app/screens/shell/route")
local Where = require("@app/screens/chat/where")
local Dock = require("@app/screens/chat/dock")
local CloudTray = require("@app/screens/chat/cloud_tray")
local LinkNotice = require("@app/screens/chat/link_notice")
local ReplyImages = require("@app/screens/chat/reply_images")
local FileLinks = require("@app/screens/chat/file_links")
local LinkChips = require("@app/screens/chat/link_chips")
local LinkMenu = require("@app/screens/chat/link_menu")
local boundary = require("@app/screens/shell/boundary")
local Quote = require("@ds/quote")

-- What its tab says, and its icon, as the shipping app's.
local M = {}

function M.title(tab: any): string
	if tab.title and tab.title ~= "" then
		return tab.title
	end
	return locale.s(if tab.kind == "subagent" then "chat.lane_tab.subagent" else "chat.lane_tab.side_chat")
end

function M.icon(_tab: any): string
	return "side-chat"
end

-- A side chat's composer: the chat page's (ds.Composer), its footer the
-- same row: the access mode, where it works, the model. Its draft is its
-- own. Its picks start as its chat has them (the engine's `harness/model`,
-- its access, its place), or, before its first message makes it, as its
-- parent's are (the shipping app's `create_child_chat` copies the parent's
-- config and place): the parent's agent and model (an agent with no model
-- named starts on that agent's first), its access, its project, device and
-- checkout. Until then where it works is a picker, as a new chat's (Cloud
-- left out: a side chat of a Cloud chat shares its machine); after, it's
-- said, fixed, as a session's is. A pick the person made stays theirs, kept
-- by the chat across tab switches and a reload, and every message goes with
-- what's picked (a changed agent, model or access becomes the chat's, as the
-- chat page's does). A message that didn't go says so on the composer's own
-- notice line, above it, never over it.
local function composer(chat: string, view: any, name: string): Node
	-- (Its draft kept by the chat, as its picks: a switch of tabs or a
	-- reload finds it as it was; a passage quoted from its transcript lands
	-- in it.)
	local draft = ui.state(`chat_lane_draft_{chat}`, "")
	-- Its draft's chips: one draft's per chat (screens/chat/shared), wherever
	-- it's kept.
	local carried = ui.use_state({} :: { any })
	ui.effect(function()
		carried:set(Shared.cargo_of(chat))
	end)
	-- The passages mentioned in its draft, by their lines (the chat page's
	-- `shared.quotes`): each goes with the message in full.
	local quotes = ui.state(`chat_lane_quotes_{chat}`, {} :: { [string]: string })
	-- What its chat asks, and its queue, todos and tasks, from its own
	-- session, as the window's composer shows them.
	local asks = Asks.bind {
		id = function(): string?
			return chat
		end,
		session = function(): any
			return view.session:get()
		end,
		chosen = ui.state(`chat_lane_chosen_{chat}`, {} :: { [string]: boolean }),
		asked = ui.state(`chat_split_asked_{chat}`, nil :: any),
		-- (A steer glides in its own transcript, as its sends do.)
		name = name,
	}
	local tray = ui.state(`chat_split_tray_{chat}`, nil :: any)
	local session = view.session
	local notice = ui.use_state(nil :: any)
	-- Its Cloud machine, on a chat that has one (a side chat shares its
	-- parent's).
	local machine = CloudTray.bind {
		chat = function(): string?
			return chat
		end,
		device = function(): string?
			local s = session:get()
			return s and s.device
		end,
		running = function(): boolean
			local s = session:get()
			return s ~= nil and s.running == true
		end,
	}
	-- A queued message edited in its composer.
	local editing = ui.state(`chat_lane_editing_{chat}`, nil :: { item: string, aside: string }?)
	local edits = Asks.queue_edits {
		id = function(): string?
			return chat
		end,
		text = draft,
		editing = editing,
		focus = function()
			focus:add(1)
		end,
		say = function(tone: string, key: string)
			notice:set({ tone = tone, text = locale.s(key), dismissable = true })
		end,
	}
	-- A tab closed mid-edit lets go of the edit, and made again finds its row
	-- no longer held: the draft it set aside back. (A row gone is another
	-- matter: it keeps the words, and saving says so.)
	ui.effect(function()
		local e = editing:get()
		local s = session:get()
		if not e or not s or s.state == "loading" then
			return
		end
		for _, row in s.queue or {} do
			if row.id == e.item and not row.editing then
				ui.batch(function()
					editing:set(nil)
					draft:set(e.aside)
				end)
			end
		end
	end)
	local running = ui.computed(function(): boolean
		local s = session:get()
		return s ~= nil and s.running == true
	end)
	-- (Kept by the chat, outside the view: a pick stays across a switch of
	-- tabs and a reload.)
	local model = ui.state(`chat_lane_model_{chat}`, nil :: string?)
	local effort = ui.state(`chat_lane_effort_{chat}`, nil :: string?)
	local access = ui.state(`chat_lane_access_{chat}`, nil :: string?)
	local project = ui.state(`chat_lane_project_{chat}`, nil :: string?)
	local device = ui.state(`chat_lane_device_{chat}`, nil :: string?)
	local checkout = ui.state(`chat_lane_checkout_{chat}`, nil :: string?)
	local branch = ui.state(`chat_lane_branch_{chat}`, nil :: string?)
	-- Where it was seeded from (its parent's place), so a send can tell
	-- whether the person moved it.
	local seed = ui.state(`chat_lane_place_{chat}`, nil :: { [string]: any }?)
	local function unsent(): boolean
		return lanes.unsent:get()[chat] ~= nil
	end
	-- Its parent: the open chat's record when that's it (its access is
	-- there), else its row in the sidebar's list.
	local function parent_of(): (any?, any?)
		local parent = lanes.unsent:get()[chat]
		if not parent then
			return nil, nil
		end
		local row: any = nil
		for _, c in sessions.now:get().chats do
			if c.id == parent then
				row = c
			end
		end
		local open = store.session:get()
		return row, if open and open.id == parent then open else nil
	end
	-- What it runs as its chat (or its parent) has it: the agent, the
	-- model's picker value, the effort, the access.
	local function as_made(): (string?, string?, string?, string?)
		local s = session:get()
		if s and s.harness then
			return s.harness, s.model, s.effort, s.access
		end
		local row, open = parent_of()
		if open and open.harness then
			return open.harness, open.model, open.effort, open.access
		end
		if row then
			local m = row.model
			if m and row.harness and not string.find(m, "/", 1, true) then
				m = `{row.harness}/{m}`
			end
			return row.harness, m, nil, nil
		end
		return nil, nil, nil, nil
	end
	-- (What was put in the pickers last from the chat: a pick that differs
	-- is the person's, and stays. Kept by the chat too, so a tab made again,
	-- or a reload, tells the person's pick from the chat's.)
	local seeded = ui.state(`chat_lane_seeded_{chat}`, {} :: { [string]: any })
	ui.effect(function()
		local harness, want, eff, acc = as_made()
		if not want and harness then
			for _, m in store.catalog:get().models do
				if m.harness == harness then
					want = m.value
					break
				end
			end
		end
		acc = acc or "full"
		local was = seeded:peek()
		local now = table.clone(was)
		-- (Never seeded: whatever stands is the chat's to replace.)
		local first = next(was) == nil
		if want then
			if (first or model:peek() == was.model) and model:peek() ~= want then
				model:set(want)
			end
			now.model = want
		end
		if (first or effort:peek() == was.effort) and effort:peek() ~= eff then
			effort:set(eff)
		end
		now.effort = eff
		if (first or access:peek() == was.access) and access:peek() ~= acc then
			access:set(acc)
		end
		now.access = acc
		if now.model ~= was.model or now.effort ~= was.effort or now.access ~= was.access then
			seeded:set(now)
		end
	end)
	-- Before its first message: where it works starts where its parent
	-- does (once, as the parent is known).
	ui.effect(function()
		if not unsent() or seed:peek() ~= nil then
			return
		end
		local row = parent_of()
		if not row then
			return
		end
		local place = {
			project = row.project or Where.NO_PROJECT,
			device = row.device,
			checkout = "local",
			branch = row.branch,
		}
		project:set(place.project)
		device:set(place.device)
		checkout:set(place.checkout)
		branch:set(place.branch)
		seed:set(place)
	end)
	local where = Where.bind {
		device = device,
		project = project,
		checkout = checkout,
		branch = branch,
		active = unsent,
		slot = `lane.{chat}`,
		remember = false,
	}
	-- (No Cloud machine of its own for a side chat.)
	local devices = ui.computed(function(): { any }
		local out = {}
		for _, d in where.devices:get() do
			if not d.cloud then
				table.insert(out, d)
			end
		end
		return out
	end)
	-- Once made: where it works, said.
	local function branch_fixed(): any
		if unsent() then
			return nil
		end
		local s = session:get()
		local function project_row(id: string?): any?
			for _, p in sessions.projects:get() do
				if p.id == id then
					return p
				end
			end
			return nil
		end
		if not s or s.harness == nil then
			-- (Just made, its chat not heard yet: where it was made.)
			local p = project_row(project:get())
			return {
				name = branch:get(),
				worktree = checkout:get() == "worktree",
				project = if p then p.name else nil,
				hue = if p then (route.look(p.id) or { hue = route.hue(p.id) }).hue else nil,
				src = if p then (route.look(p.id) or {}).src else nil,
				emoji = if p then (route.look(p.id) or {}).emoji else nil,
			}
		end
		local p = project_row(s.project)
		local on: string? = nil
		for _, d in sessions.devices:get() do
			if d.id == s.device then
				on = d.name
			end
		end
		return {
			name = s.branch,
			worktree = p ~= nil and s.cwd ~= nil and s.cwd ~= p.path,
			project = if p then p.name else nil,
			hue = if p then (route.look(p.id) or { hue = route.hue(p.id) }).hue else nil,
			src = if p then (route.look(p.id) or {}).src else nil,
			emoji = if p then (route.look(p.id) or {}).emoji else nil,
		}
	end
	-- What a message goes with: the pick, its agent, the effort, the access.
	local function picks(): { [string]: any }
		local m = model:peek()
		local out: { [string]: any } = { model = m, effort = effort:peek(), access = access:peek() }
		for _, c in store.catalog:peek().models do
			if c.value == m then
				out.harness = c.harness
			end
		end
		return out
	end
	-- Where its first message makes it: nil, its parent's place as it is;
	-- else where the person moved it (`where.plan()`).
	local function place(): { [string]: any }?
		local was = seed:peek()
		if
			was
			and project:peek() == was.project
			and device:peek() == was.device
			and checkout:peek() == was.checkout
			and branch:peek() == was.branch
		then
			return nil
		end
		return where.plan() :: any
	end
	-- (A chat sent already has its host in the session: `device` is only
	-- seeded while it's unsent.)
	local accounts = Shared.model_accounts(function(): string?
		if unsent() then
			return device:get()
		end
		local s = session:get()
		return if s then s.device else nil
	end)
	-- A new side chat's composer takes the keys as it's made.
	if lanes.take_keys:peek()[chat] then
		ui.on_mount(function()
			local left = table.clone(lanes.take_keys:peek())
			left[chat] = nil
			lanes.take_keys:set(left)
			focus:add(1)
		end)
	end
	-- What `@`, `/` and `$` offer, as the chat page's.
	local open_model = ui.use_state(0)
	local function run_command(command: string)
		if command == "model" then
			open_model:add(1)
		else
			ZeronCommands.run(command, if unsent() then nil else chat)
		end
	end
	local completions = Completions.bind {
		key = `chat_lane_{chat}`,
		text = draft,
		agent = function(): string?
			return Completions.agent_of(model:get(), if unsent() then nil else session:get())
		end,
		target = function(): any
			if unsent() then
				local p = project:get()
				return if p and p ~= Where.NO_PROJECT then { space = p, device = device:get() } else nil
			end
			local s = session:get()
			return { chat = chat, device = if s then s.device else nil }
		end,
		files = function(): any
			if unsent() then
				local p = project:peek()
				return if p and p ~= Where.NO_PROJECT then { space = p, device = device:peek() } else nil
			end
			local s = session:peek()
			return { chat = chat, device = if s then s.device else nil }
		end,
		in_chat = function(): boolean
			return not unsent()
		end,
		run = run_command,
	}
	local dictation = Dictation.bind(draft, function(): string
		local edit = editing:get()
		return `{chat}|{name}|{if edit then edit.item else ""}`
	end, function(): boolean
		local current = session:get()
		return asks.question:get() ~= nil or asks.approval:get() ~= nil
			or (current ~= nil and (current.state == "missing" or current.state == "offline"))
	end, name)
	return ds.Composer {
		dictation = dictation,
		cargo = carried,
		-- (Its chips' × under the pointer, as the chat page's.)
		on_remove_cargo = function(cid: string)
			Shared.remove_cargo_of(chat, cid)
		end,
		-- (What's dropped on it is its tab's to take, under its own overlays.)
		drop_overlay = false,
		-- Its + and a picture pasted: chips on its own draft, as the chat page's.
		on_attach = function()
			ui.open_files({ title = ui.t("chat.composer.attach_files"):get(), multiple = true }):after(function(files: any)
				if files and #files > 0 then
					Shared.attach_files(files, chat)
					focus:add(1)
				end
			end, function() end)
		end,
		on_paste_image = function(): boolean
			store.paste_image():after(function(path: string?)
				if type(path) == "string" and path ~= "" then
					Shared.attach_files({ { name = "image.png", path = path } }, chat)
					focus:add(1)
				end
			end, function() end)
			return true
		end,
		focus = focus,
		value = draft,
		-- (ds.Composer's placeholder is a string: read as the tab is made.)
		placeholder = locale.s("chat.lane_tab.placeholder"),
		running = running,
		access = access,
		model = model,
		effort = effort,
		fast = fast,
		options = options,
		open_model = open_model,
		-- The app's settings: what sends, and whether Escape stops the agent.
		send_with = function(): string
			return prefs.send_with:get()
		end,
		on_escape = function(): boolean
			return store.escape_chat(chat, running:peek())
		end,
		disabled = function(): boolean
			local s = session:get()
			return s ~= nil and (s.state == "missing" or s.state == "offline")
		end,
		mentions = completions.lists,
		on_mention = completions.on_mention,
		mention_enabled = completions.enabled,
		mention_empty = completions.empty,
		-- (Its own line first, else its Cloud machine's, on a chat with one.)
		notice = ui.computed(function(): any
			local mine = notice:get()
			if mine then
				return mine
			end
			local link = machine.cloud_link(string.find(draft:get(), "%S") ~= nil)
			return if link then LinkNotice.of { cloud = link } else nil
		end),
		on_dismiss_notice = function()
			notice:set(nil)
		end,
		on_notice_action = machine.wake,
		alerts = machine.alerts,
		on_alert_action = machine.on_alert,
		on_dismiss_alert = machine.dismiss,
		harnesses = function(): { any }
			return store.catalog:get().harnesses
		end,
		models = function(): { any }
			return store.catalog:get().models
		end,
		accounts = accounts.accounts,
		account_group = accounts.account_group,
		on_account = accounts.on_account,
		on_accounts_open = accounts.on_accounts_open,
		-- What its context holds: the chat page's ring and card, from the
		-- side chat's own numbers (once it has run and they're reported).
		usage = function(): any
			local s = session:get()
			if not s or (s.context_tokens == nil and s.context_window == nil) then
				return nil
			end
			return { tokens = s.context_tokens, window = s.context_window }
		end,
		project = project,
		projects = where.projects,
		device = device,
		devices = devices,
		checkout = checkout,
		branch = branch,
		branches = where.branches,
		branches_empty = where.branches_empty,
		branch_fixed = branch_fixed,
		on_send = function(text: string)
			local words = string.match(text, "^%s*(.-)%s*$") or ""
			if words == "" then
				return
			end
			-- (Nowhere it can start, a project gone: the line says why.)
			local is_unsent = lanes.unsent:peek()[chat] ~= nil
			if is_unsent and where.blocked:peek() then
				notice:set({ tone = "danger", text = where.blocked:peek(), dismissable = true })
				return
			end
			draft:set("")
			notice:set(nil)
			-- What it carries goes with it, as the chat page's does.
			local chips = Shared.take_cargo_of(chat)
			local pack = Carried.pack(chips, words, words)
			-- (Each command or skill picked: its canonical reference.)
			-- (And each reference to what it carries, `@` picked: its name.)
			local typed = Carried.settle(Invocations.encode(pack.typed), chips)
			local prompt = Carried.text(typed, pack)
			-- As the chat page sends: the prompt joins its transcript as the
			-- transcript's send steps clear of the end (it glides to the top,
			-- its reply's room opening under it), then goes to the host with
			-- that id, so the host's copy is the same row.
			-- (While a turn runs it's queued instead: it shows in the queue, and
			-- what's picked waits for the turn it will run in.)
			local queued = not is_unsent and running:peek()
			local req: { [string]: any } = if queued then {} else picks()
			req.attachments = pack.attachments
			req.appshots = Appshots.of_cargo(chips)
			local function failed(err: any)
				ui.batch(function()
					draft:set(text)
					quotes:set(said_quotes)
				end)
			end
			if queued then
				deliver(nil)
				return
			end
			-- As the chat page sends: the prompt joins its transcript as the
			-- transcript's send steps clear of the end (it glides to the top,
			-- its reply's room opening under it), then goes to the host with
			-- that id, so the host's copy is the same row.
			local mid = store.new_id()
			T.send(function()
				local here = false
				for _, m in view.messages:peek() do
					if m.id == mid then
						here = true
					end
				end
				if not here then
					-- (On its way from this moment: the line under it says so.)
					view.messages:push { id = mid, role = "user", text = words, waiting = "sending" }
				end
				deliver(mid)
			end, mid, name)
		end,
		on_stop = function()
			store.interrupt(chat)
		end,
	}
end

-- A fork's tab while the engine copies it, or once it couldn't: the
-- shipping app's words, and Retry (the same fork again).
local function forking(chat: string): Node
	return ui.presence(function(): string?
		local f = lanes.forks:get()[chat]
		return if f then f.state else nil
	end, function(state: string): Node
		if state == "failed" then
			return ds.EmptyState {
				icon = "git-branch",
				size = "sm",
				fill = true,
				title = ui.t("chat.lane_tab.couldnt_fork") :: any,
				body = function(): string
					local f = lanes.forks:get()[chat]
					return if f and f.error then f.error else ""
				end,
				action = {
					label = ui.t("chat.lane_tab.retry") :: any,
					on_click = function()
						lanes.retry_fork(chat)
					end,
				},
			}
		end
		return ds.EmptyState { icon = "git-branch", size = "sm", fill = true, title = ui.t("chat.lane_tab.forking") :: any }
	end, { grow = 1, w_full = true })
end

M.Tab = ui.component(function(props: { tab: any }): Node
	local tab = props.tab
	local slot, chat = tab.id, tab.chat
	local data = tab.data or {}
	local view = lanes.view(slot)
	-- What the tab is now: a fork under way or failed, or its chat (a new
	-- side chat's not yet made, until its first message).
	local kind = ui.computed(function(): string
		local f = lanes.forks:get()[chat]
		if f then
			return "fork"
		end
		return if lanes.unsent:get()[chat] then "unsent" else "chat"
	end)
	-- Watched once there's a chat to watch, while its tab is open: the
	-- panel lets it go as the tab closes. (A reload asks for the same chat
	-- again, which the host already has: nothing more happens.)
	ui.effect(function()
		if kind:get() == "chat" then
			lanes.watch(slot, chat, data.frozen == true, data.parent)
		end
	end)
	-- The transcript as the chat page has it (screens/chat/session), its
	-- composer docked over its foot the same way (screens/chat/dock): the
	-- send's runway and glide, the working row and the jump-to-latest pill
	-- held clear of the composer, its place kept across a switch of tabs
	-- and a reload (its anchor and offset kept by the tab), pictures read
	-- from its own folder, links, quotes into its own draft.
	local offset = ui.state(`chat_lane_offset_{slot}`, T.PAST_END)
	local anchor = ui.state(`chat_lane_anchor_{slot}`, nil :: any)
	local dock_h = ui.state(`chat_lane_dock_h_{slot}`, Dock.REST)
	local room = Dock.room(function(): number
		return dock_h:get()
	end)
	local function lane_session(): any
		local s = view.session:get()
		return if s and s.state ~= "missing" then s else nil
	end
	local pictures = ReplyImages.for_session(lane_session)
	local sent_pictures = ReplyImages.for_attachments(function(): any
		local s = lane_session()
		if s then
			return { id = s.id, device = s.device, cwd = s.cwd, here = sessions.here:get() }
		end
		-- A subagent's lane has no chat of its own (its doc is its parent's
		-- child): the pictures its tools kept are files on its parent's
		-- host, read as the parent's are.
		local parent = data.parent
		local open = store.session:get()
		if not parent or not open or open.id ~= parent or open.state == "missing" then
			return nil
		end
		return { id = open.id, device = open.device, cwd = open.cwd, here = sessions.here:get() }
	end)
	-- The session whose panel it's in (the page's): where a file it links
	-- opens, and a subagent it spawned.
	local function home_session(): string?
		local s = route.session:peek()
		return if s then s.id else nil
	end
	local function open_link(target: string)
		local other = string.match(target, "^zeron%-chat:([%w%-_]+)$")
		if other then
			route.open_session(other)
			return
		end
		local s = view.session:peek()
		local r = FileLinks.resolve(target, if s then s.cwd else nil)
		local panel: any = boundary.load("@app/screens/panel")
		local home = home_session()
		if r and not r.outside and panel and panel.open_file and home then
			(panel.open_file :: any)(home, r.path, route.project:peek(), { line = r.line, column = r.column, end_line = r.end_line })
		end
	end
	-- A passage quoted (its menu's Mention, or Attach): into its own draft.
	local draft = ui.state(`chat_lane_draft_{chat}`, "")
	local function quote(q: any)
		draft:set(Quote.add(draft:peek(), Quote.line(q)))
	end
	local transcript = T.Transcript {
		-- (Its own glide, runway and place: never the chat page's.)
		name = `lane.{slot}`,
		measure = function(): number
			return prefs.conversation_width:get()
		end,
		messages = view.messages,
		draft = view.draft,
		offset = offset,
		keep = anchor,
		foot = if tab.kind == "subagent" then nil else room.foot,
		compact = function(): boolean
			return prefs.compact_transcript:get()
		end,
		on_mention = quote,
		-- (Attached (its bar's Attach): a chip, as the chat page's.)
		on_attach = function(q: any)
			Shared.attach_quote(q, chat)
			focus:add(1)
		end,
		turn_state = if tab.kind == "subagent" then nil else TurnGlue.of(function(): any
			return view.session:get()
		end),
		on_retry = function(message: string)
			store.retry(chat, message)
		end,
		pictures = pictures,
		sent_pictures = sent_pictures,
		on_open_subagent = function(sub: any)
			local home = home_session()
			if home then
				lanes.open_subagent(home, sub)
			end
		end,
		open_link = open_link,
		link_menu = LinkMenu(open_link, function(): any
			return view.session:peek()
		end),
		-- (A reply's link to one of its files or its agent's skills: a chip.)
		link_chip = LinkChips(function(): any
			return view.session:peek()
		end),
	}
	if tab.kind == "subagent" then
		return ui.column { id = `lane-{slot}`, class = "lane-tab subagent-tab", provide = { session = chat }, grow = 1, w_full = true, min_h = 0, transcript }
	end
	return ui.column {
		id = `lane-{slot}`,
		class = "lane-tab side-chat-tab",
		provide = { session = chat },
		grow = 1,
		w_full = true,
		min_h = 0,
		-- (A real swap: the fork's word, then its chat.)
		ui.presence(function(): string
			return if kind:get() == "fork" then "fork" else "chat"
		end, function(k: string): Node
			if k == "fork" then
				return forking(chat)
			end
			-- As the chat page: the transcript to the composer's footer line,
			-- running on behind the composer, which floats at the foot.
			return ui.column {
				grow = 1,
				w_full = true,
				min_h = 0,
				relative = true,
				ui.column {
					w_full = true,
					h_full = true,
					min_h = 0,
					pb = Dock.page_pad(),
					ui.column { w_full = true, grow = 1, min_h = 0, relative = true, transcript },
				},
				Dock.Dock { height = dock_h, composer = composer(chat, view, `lane.{slot}`), id = `lane-composer-{slot}` },
			}
		end, { grow = 1, w_full = true, min_h = 0 }),
	}
end)

return M
