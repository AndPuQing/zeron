-- The offset debounce runs on wall time. Scroll layout can restart it while
-- the test delivers frames, so wait for the saved position before moving on.
return function(app: any)
	for _ = 1, 10 do
		if app:state("chat.transcript.anchor") ~= nil then
			return
		end
		app:wait("#never-shown", { timeout_ms = 100 })
	end
	assert(app:state("chat.transcript.anchor") ~= nil, "the transcript did not save its reading position")
end
