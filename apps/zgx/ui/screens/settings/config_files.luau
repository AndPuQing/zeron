local ds = require("@ds")
local host = require("@app/screens/settings/host")

type File = { label: string, path: string, exists: boolean }
type Reply = { files: { File }, note: string? }
export type Target = { harness: string, device: string?, label: string, device_name: string }

local function t(key: string, fields: any?): Message
	return ui.t(`settings.config_files.{key}`, fields)
end

return function(target: Target, close: () -> ()): Node
	local open = ui.use_state(true)
	local result = ui.use_state(nil :: Reply?)
	local loading = ui.use_state(false)
	local problem = ui.use_state("")
	ui.effect(function()
		if not open:get() then close() end
	end)

	local function load()
		loading:set(true)
		result:set(nil)
		problem:set("")
		host.call("settings.engine", {
			method = "ListHarnessConfigFiles",
			params = { harness = target.harness, targetDeviceId = target.device },
		}):after(function(reply: Reply)
			if not open:peek() then return end
			result:set(reply)
			loading:set(false)
		end, function(error: any)
			if not open:peek() then return end
			loading:set(false)
			local message = if type(error) == "table" then error.message else nil
			problem:set(if message == "outdated" then t("outdated"):get() else t("load_error"):get())
		end)
	end
	load()

	local files = ui.computed(function(): { File }
		local reply = result:get()
		return if reply then reply.files else {}
	end)
	local function note(): string
		local reply = result:get()
		return if reply then reply.note or "" else ""
	end
	return ds.Sheet {
		open = open,
		title = t("title"),
		description = t("target", { agent = target.label, device = target.device_name }),
		size = 560,
		ui.show(loading, ui.text { t("loading"), color = "$text.secondary" }),
		ui.show(function() return problem:get() ~= "" end, ui.text { problem, color = "$text.secondary", wrap = true, w_full = true }),
		ui.show(function() return note() ~= "" end, ui.text { note, color = "$text.secondary", wrap = true, w_full = true }),
		ui.each(files, function(_file: File, _index: number, row: State<File>): Node
			return ds.Settings.Row {
				label = function()
					local file = row:get()
					return if file.exists then file.label else t("missing", { label = file.label }):get()
				end,
				description = function() return row:get().path end,
				control = ui.row {
					gap = "$space.2",
					ds.Button { t("copy"), size = "sm", on_click = function() ui.clipboard.write(row:peek().path) end },
					if target.device == nil then ui.show(function() return row:get().exists end, function()
						return ds.Button {
							ui.t("common.open"), size = "sm",
							on_click = function()
								problem:set("")
								host.call("provider_config.open", { harness = target.harness, path = row:peek().path }):after(function() end, function()
									if open:peek() then problem:set(t("open_error"):get()) end
								end)
							end,
						}
					end) else nil,
				},
			}
		end, { key = function(file: File) return file.label end, w_full = true }),
		foot = ui.row {
			gap = "$space.2",
			ds.Button { t("refresh"), disabled = loading, on_click = load },
			ds.Button { ui.t("common.close"), on_click = function() open:set(false) end },
		},
	}
end
