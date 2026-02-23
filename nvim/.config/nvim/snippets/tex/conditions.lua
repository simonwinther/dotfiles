if ... ~= "_load" then
  return {}
end

local M = {}

function M.in_mathzone()
  return vim.fn["vimtex#syntax#in_mathzone"]() == 1
end

function M.not_in_mathzone()
  return vim.fn["vimtex#syntax#in_mathzone"]() == 0
end

return M
