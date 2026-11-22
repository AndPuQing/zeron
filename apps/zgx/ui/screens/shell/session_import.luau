local ds = require("@ds")
local sessions = require("@account/sessions")
local route = require("@app/screens/shell/route")
local host = require("@app/screens/settings/host")
local toast = require("@ds/toast")

local M = {}
local open = ui.state("session_import_open", false)
local snapshot = ui.shared("session_import.snapshot", nil)
local providers = ui.shared("session_import.providers", {})

local function t(key: string, fields: any?): Message
	return ui.t("shell.session_import." .. key, fields)
end

local function update(action: string, value: any?)
	host.invoke("session_import.update", { action = action, value = value })
end

function M.open()
	host.call("session_import.open"):after(function()
		open:set(true)
	end, function(error: any)
		toast.show {
			title = t("unavailable"),
			body = type(error) == "table" and error.message or nil,
			tone = "danger",
		}
	end)
end

local function view(): any
	return snapshot:get() or {}
end

local function busy(): boolean
	return view().running == true or view().loading == true
end

local function message(text: () -> string): Node
	return ui.text { text, color = "$text.secondary", wrap = true, w_full = true }
end

local function results(): Node
	return ui.each(ui.computed(function()
		return view().results or {}
	end), function(_: any, _index: number, row: State<any>): Node
		return ui.column {
			w_full = true,
			gap = "$space.1",
			ui.row {
				w_full = true,
				items = "center",
				gap = "$space.2",
				ui.text {
					function()
						local result = row:get()
						local phase = t("phase." .. result.phase):get()
						return result.title .. " · " .. phase
					end,
					grow = 1, min_w = 0, truncate = true,
				},
				ui.show(function()
					return row:get().chat_id ~= nil
				end, function()
					return ds.Button {
						t("open_chat"),
						size = "sm",
						on_click = function()
							local id = row:peek().chat_id
							if id then
								open:set(false)
								route.open_session(id)
							end
						end,
					}
				end),
			},
			message(function()
				return row:get().reason or ""
			end),
		}
	end, { key = function(result: any) return result.source_ref end, w_full = true, gap = "$space.2" })
end

function M.Sheet(): Node
	local device = ui.use_state("")
	local provider = ui.use_state("")
	local query = ui.use_state("")
	ui.effect(function()
		local current = view()
		device:set(current.device_id or "")
		provider:set(current.provider or "")
		query:set(current.query or "")
	end)
	ui.effect(function()
		if open:get() then
			sessions.devices:get()
			update("hosts")
		else
			host.invoke("session_import.close")
		end
	end)
	ui.on_cleanup(function()
		host.invoke("session_import.close")
		open:set(false)
	end)

	local function hosts(): { any }
		local options = {}
		for _, item in view().hosts or {} do
			local why = if not item.online then t("offline"):get()
				elseif not item.supported then t("outdated"):get()
				else nil
			table.insert(options, { value = item.id, label = item.name, description = why, disabled = why ~= nil })
		end
		return options
	end

	return ds.Sheet {
		open = open,
		title = t("title"),
		size = 560,
		ui.row {
			w_full = true, gap = "$space.3",
			ui.column {
				grow = 1, min_w = 0,
				ds.Select {
					label = t("device"), value = device, options = hosts, w_full = true,
					disabled = function() return view().running == true end,
					on_change = function(id: string) update("device", id) end,
				},
			},
			ui.column {
				grow = 1, min_w = 0,
				ds.Select {
					label = t("agent"), value = provider, w_full = true,
					options = function()
						local options = { { value = "", label = t("all_agents"):get() } }
						for _, choice in providers:get() do
							table.insert(options, choice)
						end
						return options
					end,
					on_change = function(id: string) update("provider", if id ~= "" then id else nil) end,
				},
			},
		},
		ds.TextField {
			value = query, label = t("search"), clearable = true, w_full = true,
			on_change = function(text: string) update("query", text) end,
		},
		ui.row {
			w_full = true, gap = "$space.2", items = "center",
			ds.Button {
				t("select_visible"), size = "sm",
				disabled = function() return busy() or #(view().rows or {}) == 0 end,
				on_click = function() update("select_visible") end,
			},
			ds.Button {
				t("refresh"), size = "sm",
				disabled = function() return busy() or view().device_id == nil end,
				on_click = function() update("refresh") end,
			},
			ui.text { function() return t("selected", { count = view().selected_count or 0 }):get() end, color = "$text.secondary" },
		},
		message(function()
			local current = view()
			if current.device_id == nil then
				return t("no_devices"):get()
			elseif current.loading then
				return t("scanning"):get()
			elseif current.discovery_error then
				return current.discovery_error
			elseif #(current.rows or {}) == 0 then
				return t("empty"):get()
			end
			return ""
		end),
		ui.each(ui.computed(function()
			return view().source_errors or {}
		end), function(error: string): Node
			return message(function() return error end)
		end, { w_full = true }),
		ui.each(ui.computed(function()
			return view().rows or {}
		end), function(initial: any, _index: number, row: State<any>): Node
			return ui.column {
				w_full = true, gap = "$space.1",
				ui.presence(function()
					return if busy() or row:get().disabled_reason ~= nil then "disabled" else "ready"
				end, function(state: string): Node
					return ds.Checkbox {
						value = function() return row:get().selected end,
						label = row:peek().title,
						disabled = state == "disabled",
						on_change = function() update("toggle", initial.source_ref) end,
					}
				end),
				message(function()
					local item = row:get()
					return item.disabled_reason or (item.provider .. " · " .. item.cwd)
				end),
			}
		end, { key = function(row: any) return row.source_ref end, w_full = true, gap = "$space.3" }),
		ui.show(function() return view().has_more == true end, function()
			return ds.Button {
				t("more"), disabled = busy,
				on_click = function() update("more") end,
			}
		end),
		results(),
		message(function() return view().error or "" end),
		foot = ui.row {
			gap = "$space.2",
			ds.Button {
				ui.t("common.close"),
				on_click = function() open:set(false) end,
			},
			ui.show(function() return view().running == true end, function()
				return ds.Button {
					t("stop"), disabled = function() return view().cancelling == true end,
					on_click = function() update("cancel") end,
				}
			end),
			ui.show(function()
				if busy() then return false end
				for _, result in view().results or {} do
					if result.retryable then return true end
				end
				return false
			end, function()
				return ds.Button { t("retry"), on_click = function() update("retry") end }
			end),
			ds.Button {
				t("import"), tone = "primary",
				disabled = function() return busy() or (view().selected_count or 0) == 0 end,
				on_click = function() update("start") end,
			},
		},
	}
end

return M
