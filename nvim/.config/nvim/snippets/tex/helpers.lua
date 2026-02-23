if ... ~= "_load" then
  return {}
end

local M = {}

function M.to_label(args)
  return args[1][1]:lower():gsub("%p", ""):gsub("%s+", "_")
end

return M
