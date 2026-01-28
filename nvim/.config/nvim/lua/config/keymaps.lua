-- Keymaps are automatically loaded on the VeryLazy event Default keymaps that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/keymaps.lua Add any additional keymaps here local opts = { noremap = true, silent = true }
local opts = { noremap = true, silent = true }

----------------------------------------
--- Helper functions
----------------------------------------
local function is_mapped(mode, lhs)
  if vim.keymap and vim.keymap.get then
    local maps = vim.keymap.get(mode, lhs)
    return maps and #maps > 0, maps
  end
end

----------------------------------------
--- Oil
----------------------------------------
vim.keymap.set("n", "-", "<cmd>Oil<cr>", { desc = "Open parent directory" })

-- -----------------------------------------
-- --- Disable mouse support
-- -----------------------------------------
-- vim.opt.mouse = ""

----------------------------------
--- Fuzzy search buffer keymap
--- The smart thing is that my flash.nvim is set to use / by default
--- So, `s` remaps to /, and `S` remaps to ?
--- But they are done before this keymap overrides /,
--- Hence `s` and `S` still work with flash.nvim (normal /)
--- But now allowing / to do fuzzy search with snacks.nvim
--- This is the exact same as LazyVim's default keymap for <leader>sb (search buffer)
----------------------------------
vim.keymap.set("n", "/", function()
  require("snacks").picker.lines()
end, { desc = "Fuzzy search buffer" })

-----------------------------------------
--- Smart Leader just to try
-----------------------------------------
vim.keymap.set("n", "<leader>se", function()
  Snacks.picker.smart()
end, { desc = "Smart Find Files" })

-----------------------------------------
--- Overwriting LazyVim's default <leader>sw and <leader>sW keymaps for Snacks grep_word with hidden files enabled
-----------------------------------------
vim.keymap.set({ "n", "x" }, "<leader>sw", function()
  Snacks.picker.grep_word({ hidden = true })
end, { desc = "Visual selection or word (Root Dir)" })

vim.keymap.set({ "n", "x" }, "<leader>sW", function()
  Snacks.picker.grep_word({ hidden = true, root = false })
end, { desc = "Visual selection or word (cwd)" })

----------------------------------------
--- Grep in Neovim config keymap
----------------------------------------
vim.keymap.set("n", "<leader>s,", function()
  require("snacks").picker.grep({
    hidden = true,
    title = "Grep Config",
    cwd = vim.fn.stdpath("config"),
  })
end, { desc = "Grep in Neovim config" })

----------------------------------------
--- Grep And Search files in Dotfiles config keymap
---------------------------------------
vim.keymap.set("n", "<leader>f.", function()
  require("snacks").picker.files({
    title = "Find Files in Dotfiles",
    cwd = "~/dotfiles",
    hidden = true,
  })
end, { desc = "Find files in Dotfiles config" })

vim.keymap.set("n", "<leader>s.", function()
  require("snacks").picker.grep({
    title = "Grep Dotfiles",
    cwd = vim.env.DOTFILES or "~/dotfiles",
    hidden = true,
  })
end, { desc = "Grep in Dotfiles config" })

----------------------------------------
--- System clipboard keymaps
----------------------------------------
-- Delete to system clipboard
vim.keymap.set({ "n" }, "<leader>dd", '"+dd', { desc = "Delete to system clipboard" })
vim.keymap.set({ "x" }, "<leader>d", '"+d', { desc = "Delete to system clipboard" })

-- Yank to system clipboard
vim.keymap.set({ "n", "x" }, "<leader>y", '"+y', { desc = "Yank to system clipboard" })
vim.keymap.set({ "n", "x" }, "<leader>Y", '"+Y', { desc = "Yank to system clipboard (line)" })

-- Paste from system clipboard
vim.keymap.set({ "n", "x" }, "<leader>p", '"+p', { desc = "Paste from system clipboard" })
vim.keymap.set({ "n", "x" }, "<leader>P", '"+P', { desc = "Paste from system clipboard" })

-- Paste the entire buffer to system clipboard
vim.keymap.set("n", "<leader>ay", function()
  local view = vim.fn.winsaveview()
  vim.cmd("silent %y+")
  vim.fn.winrestview(view)
end, { desc = "All Yank" })

-- Paste from system clipboard to entire buffer
vim.keymap.set("n", "<leader>ap", function()
  local view = vim.fn.winsaveview()
  vim.cmd("silent %delete")
  vim.cmd("silent 0put +")
  vim.cmd("silent $delete _")
  vim.fn.winrestview(view)
end, { desc = "All Paste" })

----------------------------------------
--- Buffer
----------------------------------------
-- Move the current buffer to the left
vim.keymap.set("n", "<A-S-h>", "<cmd>BufferLineMovePrev<cr>", { desc = "Move buffer left" })

-- Move the current buffer to the right
vim.keymap.set("n", "<A-S-l>", "<cmd>BufferLineMoveNext<cr>", { desc = "Move buffer right" })

----------------------------------------
--- Move lines keymaps
----------------------------------------
-- Move selected line / block of text in visual mode
vim.keymap.set("v", "J", ":m '>+1<CR>gv=gv", opts)
vim.keymap.set("v", "K", ":m '<-2<CR>gv=gv", opts)

----------------------------------------
--- Window management keymaps
----------------------------------------
-- Save and quit window with <leader>ww if not already mapped
local exists = is_mapped("n", "<leader>ww")
if not exists then
  vim.keymap.set("n", "<leader>ww", "<cmd>wq<cr>", { desc = "Save and Quit Window" })
else
  print("Mapping <leader>ww already exists")
end

-----------------------------------------
--- Visual Selection keymap
-----------------------------------------
vim.keymap.set("n", "gb", "`[v`]", { desc = "Select last pasted text" })

----------------------------------------
--- Search and Find in Home Directory (~)
----------------------------------------
-- Find Files in Home
vim.keymap.set("n", "<leader>f~", function()
  Snacks.picker.files({
    cwd = "~",
    hidden = true,
    title = "Find Files (Home)",
  })
end, { desc = "Find Files (Home)" })

-- Grep in Home
vim.keymap.set("n", "<leader>s~", function()
  Snacks.picker.grep({
    cwd = "~",
    hidden = true,
    title = "Grep (Home)",
  })
end, { desc = "Grep (Home)" })

---------- End of File ----------
