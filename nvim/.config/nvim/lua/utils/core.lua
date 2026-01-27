local M = {}

M.get_visual_selection = function()
  vim.cmd("normal! \27") -- Escape visual mode to update marks
  local _, start_line, _, _ = unpack(vim.fn.getpos("'<"))
  local _, end_line, _, _ = unpack(vim.fn.getpos("'>"))
  return vim.api.nvim_buf_get_lines(0, start_line - 1, end_line, false)
end

return M
