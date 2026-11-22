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

local function in_code_block(parser, row, col)
  local node = parser:named_node_for_range({ row, col, row, col })
  while node do
    if node:type() == "fenced_code_block" or node:type() == "indented_code_block" then
      return true
    end
    node = node:parent()
  end
  return false
end

local function markdown_math_zone()
  local cursor = vim.api.nvim_win_get_cursor(0)
  local row, col = cursor[1] - 1, cursor[2]
  local range = { row, math.max(0, col - 1), row, col }
  local parser = vim.treesitter.get_parser(0, "markdown", { error = false })
  if not parser then
    return nil
  end
  parser:parse(range)
  local inline = parser:children().markdown_inline
  local node = inline and inline:named_node_for_range(range)
  while node do
    if node:type() == "latex_block" then
      local start_row, start_col = node:start()
      local opener = vim.fn.getline(start_row + 1):sub(start_col + 1, start_col + 2)
      return opener == "$$" and "display" or "inline"
    end
    node = node:parent()
  end
  -- Blank lines split Markdown paragraphs, including the inline parser's math nodes.
  if in_code_block(parser, row, math.max(0, col - 1)) then
    return nil
  end
  local inside = false
  for index, line in ipairs(vim.api.nvim_buf_get_lines(0, 0, row, false)) do
    local marker = line:find("%S")
    if marker and line:match("^%s*%$%$%s*$") and not in_code_block(parser, index - 1, marker - 1) then
      inside = not inside
    end
  end
  return inside and "display" or nil
end

function M.in_display_math(line_to_cursor)
  if not line_to_cursor:match("^%s*align$") and not line_to_cursor:match("%$%$%s*align$") then
    return false
  end
  return markdown_math_zone() == "display"
end

function M.in_mathzone()
  if vim.bo.filetype == "markdown" then
    return markdown_math_zone() ~= nil
  end
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

function M.in_plain_mathzone(line_to_cursor, trigger)
  -- Leave literal LaTeX commands alone when typing or editing them.
  if trigger and line_to_cursor:sub(-#trigger - 1) == "\\" .. trigger then
    return false
  end
  return M.in_mathzone() and not in_text_like_math_command()
end

return M
