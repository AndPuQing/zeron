local ds = require("@ds")
local host = require("@app/screens/settings/host")

export type Target = { harness: string, device: string?, label: string, device_name: string }
type Change = { name: string, action: string, value: string? }

local function t(key: string, fields: any?): Message
	return ui.t(`settings.environment.{key}`, fields)
end

return function(target: Target, close: () -> ()): Node
	local open = ui.use_state(true)
	local metadata = ui.use_state(nil :: any)
	-- Values exist only in this editor's draft and are cleared when it closes.
	local changes = ui.use_state({} :: { [string]: Change })
	local name = ui.use_state("")
	local value = ui.use_state("")
	local operation = ui.use_state("set")
	local revealed = ui.use_state(nil :: string?)
	local pending = ui.use_state(false)
	local needs_reload = ui.use_state(false)
	local problem = ui.use_state("")
	local alive = true
	ui.on_cleanup(function()
		alive = false
	end)
	ui.effect(function()
		if not open:get() then
			value:set("")
			revealed:set(nil)
			changes:set({})
			close()
		end
	end)

	local function params(extra: { [string]: any }?): { [string]: any }
		local request = table.clone(extra or {})
		request.harness = target.harness
		request.targetDeviceId = target.device
		return request
	end

	local function fail(error: any, fallback: string)
		if alive and open:peek() then
			pending:set(false)
			problem:set(if type(error) == "table" and type(error.message) == "string" then error.message else t(fallback):get())
		end
	end

	local function load()
		pending:set(true)
		problem:set("")
		revealed:set(nil)
		host.call("environment.get", params()):after(function(reply: any)
			if alive and open:peek() then
				metadata:set(reply)
				pending:set(false)
				needs_reload:set(false)
			end
		end, function(error: any)
			fail(error, "load_error")
		end)
	end
	load()

	local function key(variable: string): string
		local current = metadata:peek()
		return if current and current.policy.caseSensitive == false then string.upper(variable) else variable
	end
	local function choose(variable: string)
		name:set(variable)
		value:set("")
		operation:set("set")
		revealed:set(nil)
	end

	local function stage()
		local variable = name:peek()
		if not string.match(variable, "^[A-Za-z_][A-Za-z0-9_]*$") then
			problem:set(t("invalid_name"):get())
			return
		end
		local action = operation:peek()
		local draft = table.clone(changes:peek())
		draft[key(variable)] = { name = variable, action = action, value = if action == "set" then value:peek() else nil }
		changes:set(draft)
		choose("")
		problem:set("")
	end

	local function save()
		local current = metadata:peek()
		local list = {}
		for _, change in changes:peek() do
			table.insert(list, change)
		end
		pending:set(true)
		problem:set("")
		revealed:set(nil)
		host.call("environment.save", params({ expectedRevision = current.revision, changes = list })):after(function(reply: any)
			if alive and open:peek() then
				pending:set(false)
				metadata:set(reply.metadata)
				if reply.conflict then
					problem:set(t("conflict"):get())
				else
					changes:set({})
					choose("")
				end
			end
		end, function(error: any)
			needs_reload:set(true)
			fail(error, "save_error")
		end)
	end

	local function reveal()
		if revealed:peek() ~= nil then
			revealed:set(nil)
			return
		end
		local revision = metadata:peek().revision
		local variable = name:peek()
		pending:set(true)
		host.call("environment.reveal", params({ name = variable, expectedRevision = revision })):after(function(reply: any)
			if alive and open:peek() then
				pending:set(false)
				if reply.revision == revision and name:peek() == variable then
					revealed:set(reply.value)
				end
			end
		end, function(error: any)
			fail(error, "reveal_error")
		end)
	end

	local function rows(): { any }
		local entries = {}
		for _, entry in (metadata:get() or {}).entries or {} do
			entries[key(entry.name)] = entry
		end
		for id, change in changes:get() do
			entries[id] = { name = change.name, action = change.action, staged = true }
		end
		local ordered = {}
		for _, entry in entries do
			table.insert(ordered, entry)
		end
		table.sort(ordered, function(a, b) return a.name < b.name end)
		return ordered
	end
	local function can_reveal(): boolean
		local selected = name:get()
		for _, entry in rows() do
			if entry.name == selected and entry.action == "set" and not entry.staged then
				return true
			end
		end
		return false
	end

	return ds.Sheet {
		open = open,
		title = t("title"),
		description = t("target", { agent = target.label, device = target.device_name }),
		size = 520,
		ui.show(function() return problem:get() ~= "" end, function()
			return ui.text { problem, color = "$text.secondary", wrap = true, w_full = true }
		end),
		ui.show(function() return needs_reload:get() or (metadata:get() == nil and not pending:get()) end, function()
			return ds.Button { t("reload"), disabled = pending, on_click = load }
		end),
		ui.show(function() return metadata:get() ~= nil end, function()
			return ui.column {
				w_full = true, gap = "$space.3",
				ui.each(ui.computed(rows), function(entry: any, _index: number, row: State<any>): Node
					return ui.row {
						w_full = true, gap = "$space.2", items = "center",
						ui.text { entry.name, literal = true, grow = 1, min_w = 0, truncate = true },
						ui.text { function() return t("action." .. row:get().action):get() end, color = "$text.secondary" },
						ds.Button { t("edit"), size = "sm", disabled = pending, on_click = function() choose(entry.name) end },
						ui.show(function() return row:get().staged == true end, function()
							return ds.Button {
								t("undo"), size = "sm", disabled = pending,
								on_click = function()
									local next = table.clone(changes:peek())
									next[key(entry.name)] = nil
									changes:set(next)
								end,
							}
						end),
					}
				end, { key = function(entry: any) return entry.name end, w_full = true, gap = "$space.2" }),
				ds.TextField {
					label = t("name"), value = name, w_full = true, disabled = pending,
					on_change = function() revealed:set(nil) end,
				},
				ds.Select {
					label = t("operation"), value = operation, w_full = true, disabled = pending,
					options = {
						{ value = "set", label = t("action.set") },
						{ value = "unset", label = t("action.unset") },
						{ value = "delete", label = t("action.delete") },
					},
				},
				ui.show(function() return operation:get() == "set" end, function()
					return ds.TextField { label = t("value"), value = value, password = true, w_full = true, disabled = pending }
				end),
				ui.show(can_reveal, function()
					return ds.Button {
						function()
							return t(if revealed:get() == nil then "show_value" else "hide_value"):get()
						end,
						disabled = pending, on_click = reveal,
					}
				end),
				ui.show(function() return revealed:get() ~= nil end, function()
					return ui.text { function() return revealed:get() or "" end, literal = true, wrap = true, w_full = true }
				end),
				ds.Button {
					t("stage"),
					disabled = function() return pending:get() or name:get() == "" end,
					on_click = stage,
				},
				ui.show(function() return #((metadata:get() or {}).previousEnvironmentSessions or {}) > 0 end, function()
					return ui.text { t("previous_sessions"), color = "$text.secondary", wrap = true, w_full = true }
				end),
			}
		end),
		foot = ui.row {
			gap = "$space.2",
			ds.Button { ui.t("common.close"), on_click = function() open:set(false) end },
			ds.Button {
				t("save"),
				tone = "primary",
				disabled = function()
					return pending:get() or needs_reload:get() or metadata:get() == nil or next(changes:get()) == nil
				end,
				on_click = save,
			},
		},
	}
end
