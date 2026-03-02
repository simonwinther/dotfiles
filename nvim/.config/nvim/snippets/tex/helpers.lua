if ... ~= "_load" then
  return {}
end

local M = {}

function M.to_label(args)
  return args[1][1]:lower():gsub("%p", ""):gsub("%s+", "_")
end

function M.smart_space(replacement)
  return function()
    local col = vim.api.nvim_win_get_cursor(0)[2]
    local line = vim.api.nvim_get_current_line()
    local next_char = line:sub(col + 1, col + 1)

    if next_char == "" or next_char:match("[%w\\]") then
      return replacement .. " "
    else
      return replacement
    end
  end
end

return M
