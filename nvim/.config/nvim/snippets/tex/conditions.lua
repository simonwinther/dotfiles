if ... ~= "_load" then
  return {}
end

local M = {}

local text_like_math_commands = {
  mathbf = true,
  mathrm = true,
  mathsf = true,
  mathtt = true,
  operatorname = true,
  text = true,
  textbf = true,
  textit = true,
  textnormal = true,
  texttt = true,
}

function M.in_mathzone()
  return vim.fn["vimtex#syntax#in_mathzone"]() == 1
end

function M.not_in_mathzone()
  return vim.fn["vimtex#syntax#in_mathzone"]() == 0
end

local function opens_text_like_math_command(line, open_brace_col)
  local command = line:sub(1, open_brace_col - 1):match("\\([%a]+)%*?%s*$")
  return text_like_math_commands[command] == true
end

local function in_text_like_math_command()
  local col = vim.api.nvim_win_get_cursor(0)[2]
  local line = vim.api.nvim_get_current_line():sub(1, col)
  local brace_stack = {}

  for i = 1, #line do
    local char = line:sub(i, i)
    local escaped = i > 1 and line:sub(i - 1, i - 1) == "\\"

    if char == "{" and not escaped then
      table.insert(brace_stack, opens_text_like_math_command(line, i))
    elseif char == "}" and not escaped and #brace_stack > 0 then
      table.remove(brace_stack)
    end
  end

  return vim.tbl_contains(brace_stack, true)
end

function M.in_plain_mathzone()
  return M.in_mathzone() and not in_text_like_math_command()
end

return M
