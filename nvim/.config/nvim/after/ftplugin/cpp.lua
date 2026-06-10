--- ====================================
--- C++ codeforces move to solutions keybind
--- ====================================
local codeforces_repo = vim.fn.expand("~/dev/codeforces-cp")
local file = vim.fn.expand("%:p")

if vim.startswith(file, codeforces_repo .. "/") and not vim.startswith(file, codeforces_repo .. "/solutions/") then
  vim.keymap.set("n", "<localleader>m", function()
    local src = vim.fn.expand("%:p")
    local src_no_ext = vim.fn.expand("%:p:r")
    local filename = vim.fn.expand("%:t")
    local solutions_dir = codeforces_repo .. "/solutions"
    local dest = solutions_dir .. "/" .. filename

    vim.fn.mkdir(solutions_dir, "p")

    if vim.fn.filereadable(dest) == 1 then
      vim.notify("File already exists in solutions: " .. filename, vim.log.levels.ERROR)
      return
    end

    if vim.fn.rename(src, dest) ~= 0 then
      vim.notify("Failed to move file", vim.log.levels.ERROR)
      return
    end

    vim.cmd("edit " .. vim.fn.fnameescape(dest))
    vim.notify("Moved to solutions/" .. filename)

    if vim.fn.filereadable(src_no_ext) == 1 then
      -- Delete the compiled executable if it exists
      vim.fn.delete(src_no_ext)
    end
  end, {
    buffer = true,
    desc = "Move file to solutions",
  })
end

-- ==============================
-- Compile and run current C++ file
-- ================================
vim.keymap.set("n", "<localleader>r", function()
  local file = vim.fn.expand("%:p")
  local exe = vim.fn.expand("%:p:r")

  vim.cmd("write")

  local cmd = string.format(
    "g++ -std=c++20 -Wall -Wextra -O2 %s -o %s && %s",
    vim.fn.shellescape(file),
    vim.fn.shellescape(exe),
    vim.fn.shellescape(exe)
  )

  vim.cmd("split | terminal " .. cmd)
  vim.cmd("startinsert")
end, {
  buffer = true,
  desc = "Compile and run C++ file",
})

vim.keymap.set("n", "<localleader>t", "<cmd>CompetiTest run<cr>", {
  buffer = true,
  desc = "Run CompetiTest",
})

--- ====================================
-- === Start of header/source switch logic ===
--- ====================================

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
-- === End of header/source switch logic ===
