-- Markview needs nowrap to align and fully render tables.
vim.opt_local.wrap = false

vim.keymap.set("n", "\\mv", "<cmd>Markview toggle<cr>", {
  desc = "Toggle Markdown preview",
  buffer = true,
})

-- Paste Image (img-clip.nvim)
vim.keymap.set("n", "\\mp", "<cmd>PasteImage<cr>", {
  desc = "[img-clip.nvim] Paste image from system clipboard",
  buffer = true,
})

-- Leaving a matrix by moving the cursor can leave its LuaSnip placeholder active.
local snippet_group = vim.api.nvim_create_augroup("MarkdownMatrixSnippet", { clear = false })
vim.api.nvim_clear_autocmds({ group = snippet_group, buffer = 0 })
vim.api.nvim_create_autocmd("InsertEnter", {
  group = snippet_group,
  buffer = 0,
  callback = function()
    local ls = package.loaded.luasnip
    if not ls then
      return
    end
    local buffer = vim.api.nvim_get_current_buf()
    local row, col = unpack(vim.api.nvim_win_get_cursor(0))
    row = row - 1
    local node = ls.session.current_nodes[buffer]
    while node and node.parent do
      local trigger = node.parent.snippet.trigger
      if trigger ~= "bmat" and trigger ~= "array" and trigger ~= "align" and trigger ~= ",M" then
        break
      end
      local first, last = node.mark:pos_begin_end_raw()
      local after_start = row > first[1] or (row == first[1] and col >= first[2])
      local before_end = row < last[1] or (row == last[1] and col <= last[2])
      if after_start and before_end then
        break
      end
      ls.unlink_current()
      node = ls.session.current_nodes[buffer]
    end
  end,
})

vim.keymap.set("i", "<CR>", function()
  local newline = _G.MiniPairs and MiniPairs.cr() or vim.keycode("<CR>")
  local line = vim.api.nvim_get_current_line()
  local content = vim.trim(line)
  if vim.fn.col(".") <= #line or content == "" or content:sub(-2) == [[\\]] then
    return newline
  end
  if content:find([[\begin{%w+}%s*$]]) or content:find([[\\%b[]$]]) or content:find("%%") then
    return newline
  end

  local boundary = vim.fn.search([[\v\\(begin|end)\{aligned\}|^\s*(\$\$|`{3,}|\~{3,})]], "bnW")
  if boundary > 0 and vim.fn.getline(boundary):find([[\begin{aligned}]], 1, true) then
    local space = line:match("%s$") and "" or " "
    return space .. [[\\]] .. newline
  end
  return newline
end, { buffer = true, expr = true, replace_keycodes = false, desc = "Continue an aligned math row" })

-- Align consecutive table rows without splitting escaped or grouped ampersands.
local function math_cells(line)
  if line:find([[\begin{]], 1, true) or line:find([[\end{]], 1, true) then
    return
  end
  local cells, start, depth, escaped = {}, 1, 0, false
  for pos = 1, #line do
    local char = line:sub(pos, pos)
    if escaped then
      escaped = false
    elseif char == "\\" then
      escaped = true
    elseif char == "%" then
      break
    elseif char == "{" then
      depth = depth + 1
    elseif char == "}" then
      depth = depth - 1
    elseif char == "&" and depth == 0 then
      cells[#cells + 1] = vim.trim(line:sub(start, pos - 1))
      start = pos + 1
    end
  end
  if #cells > 0 and depth == 0 then
    cells[#cells + 1] = line:sub(start):gsub("^%s+", "")
    return cells
  end
end

vim.keymap.set("n", "\\ma", function()
  local first = vim.fn.line(".")
  if not math_cells(vim.fn.getline(first)) then
    vim.notify("Place the cursor on a math row containing &")
    return
  end
  while first > 1 and math_cells(vim.fn.getline(first - 1)) do
    first = first - 1
  end
  local rows, widths = {}, {}
  local indent = vim.fn.getline(first):match("^%s*")
  local row = math_cells(vim.fn.getline(first))
  while row do
    rows[#rows + 1] = row
    for col = 1, #row - 1 do
      widths[col] = math.max(widths[col] or 0, vim.fn.strdisplaywidth(row[col]))
    end
    row = math_cells(vim.fn.getline(first + #rows))
  end
  local lines = {}
  for _, cells in ipairs(rows) do
    for col = 1, #cells - 1 do
      cells[col] = cells[col] .. string.rep(" ", widths[col] - vim.fn.strdisplaywidth(cells[col]))
    end
    lines[#lines + 1] = indent .. table.concat(cells, " & ")
  end
  vim.api.nvim_buf_set_lines(0, first - 1, first - 1 + #rows, false, lines)
end, { buffer = true, desc = "Align math columns" })
