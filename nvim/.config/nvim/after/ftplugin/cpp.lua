vim.keymap.set("n", "<leader>a", "<cmd>Assistant<cr>", {
  buffer = true,
  desc = "Assistant.nvim",
})

-- === Start of header/source switch logic ===

local function switch_header_source()
  local current_file = vim.fn.expand("%:p")
  local current_dir = vim.fn.expand("%:p:h")
  local filename = vim.fn.expand("%:t:r")
  local ext = vim.fn.expand("%:e")

  ---@type string[]
  local target_exts

  if ext == "h" or ext == "hpp" or ext == "hh" then
    target_exts = { "cpp", "cc", "cxx", "c" }
  elseif ext == "cpp" or ext == "cc" or ext == "cxx" or ext == "c" then
    target_exts = { "h", "hpp", "hh" }
  else
    vim.notify("Not a C/C++ source or header file", vim.log.levels.WARN)
    return
  end

  -- Try to find an existing matching file under cwd
  for _, target_ext in ipairs(target_exts) do
    local found = vim.fn.findfile(filename .. "." .. target_ext, "**")

    if type(found) == "string" and found ~= "" then
      local found_path = vim.fn.fnamemodify(found, ":p")
      vim.cmd("edit " .. vim.fn.fnameescape(found_path))
      return
    end
  end

  -- If no matching file exists, ask whether to create one
  ---@type string[]
  local choices = {}

  for _, target_ext in ipairs(target_exts) do
    table.insert(choices, filename .. "." .. target_ext)
  end

  table.insert(choices, "Cancel")

  vim.ui.select(choices, {
    prompt = "No matching header/source found. Create one?",
  }, function(choice)
    if type(choice) ~= "string" or choice == "Cancel" then
      return
    end

    local new_file = current_dir .. "/" .. choice

    vim.cmd("edit " .. vim.fn.fnameescape(new_file))

    -- If creating a source file from a header, add the include automatically
    if ext == "h" or ext == "hpp" or ext == "hh" then
      local header_name = vim.fn.fnamemodify(current_file, ":t")

      vim.api.nvim_buf_set_lines(0, 0, -1, false, {
        '#include "' .. header_name .. '"',
        "",
      })
    end
  end)
end

vim.keymap.set("n", "gm", switch_header_source, {
  desc = "Open or create matching header/source from cwd",
  cpp,
})
