local buffer = vim.api.nvim_get_current_buf()
local ns = vim.api.nvim_create_namespace("markdown_math")
local group = vim.api.nvim_create_augroup("MarkdownMathDiagnostics", { clear = false })
vim.api.nvim_clear_autocmds({ group = group, buffer = buffer })

local function check_math()
  if vim.bo[buffer].filetype ~= "markdown" then
    vim.diagnostic.reset(ns, buffer)
    return
  end
  local lines = vim.api.nvim_buf_get_lines(buffer, 0, -1, false)
  local parser = vim.treesitter.get_parser(buffer, "markdown", { error = false })
  if parser then parser:parse() end
  local diagnostics = {}
  local function report(row, col, width, severity, message)
    diagnostics[#diagnostics + 1] = {
      lnum = row - 1, col = col - 1, end_col = col - 1 + width,
      severity = severity, source = "Markdown math", message = message,
    }
  end

  local function check_block(first, last)
    local braces, pending_arrow = {}, false
    local environments = 0
    for row = first, last do
      local line = lines[row]
      if line:match("^%s*$") and (row == first or lines[row - 1]:find("%S")) then
        report(row, 1, #line, vim.diagnostic.severity.WARN,
          "Blank line inside display math can split the Markdown preview. Remove it.")
      end
      local i = 1
      while i <= #line do
        local char = line:sub(i, i)
        if char == "%" then
          break
        elseif char == "\\" then
          local command = line:match("^\\([%a]+)", i)
          if line:sub(i, i + 1) == [[\\]] and braces[#braces] == environments then
            report(row, i, 2, vim.diagnostic.severity.ERROR,
              [[Row break inside an arrow label. Move \\ after the closing } or use \substack{...}.]])
          end
          if command == "begin" then environments = environments + 1 end
          if command == "end" then environments = environments - 1 end
          pending_arrow = command == "xrightarrow" or command == "xleftarrow"
          i = i + (command and #command + 1 or 2)
        else
          if char == "{" then
            braces[#braces + 1] = pending_arrow and environments or false
          elseif char == "}" then
            braces[#braces] = nil
          end
          if char:find("%S") then pending_arrow = false end
          i = i + 1
        end
      end
    end
  end

  local opening
  for row, line in ipairs(lines) do
    local col = line:match("^%s*()%$%$%s*$")
    if col then
      local node = parser and parser:named_node_for_range({ row - 1, col - 1, row - 1, col + 1 })
      while node and node:type() ~= "fenced_code_block" and node:type() ~= "indented_code_block" do
        node = node:parent()
      end
      if not node then
        if opening then
          check_block(opening + 1, row - 1)
          opening = nil
        else
          opening = row
        end
      end
    end
  end
  vim.diagnostic.set(ns, buffer, diagnostics)
end

vim.api.nvim_create_autocmd({ "InsertLeave", "TextChanged", "BufWritePost", "FileType" }, {
  group = group, buffer = buffer, callback = check_math,
})
check_math()
