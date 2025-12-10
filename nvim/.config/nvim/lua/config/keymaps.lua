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

-----------------------------------------
--- Disable mouse support
-----------------------------------------
vim.opt.mouse = ""

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
vim.keymap.set("n", "<leader>fC", function()
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
    cwd = vim.env.DOTFILES or "~/.dotfiles",
    hidden = true,
  })
end, { desc = "Find files in Dotfiles config" })

vim.keymap.set("n", "<leader>f:", function()
  require("snacks").picker.grep({
    title = "Grep Dotfiles",
    cwd = vim.env.DOTFILES or "~/.dotfiles",
    hidden = true,
  })
end, { desc = "Grep in Dotfiles config" })

----------------------------------------
--- System clipboard keymaps
----------------------------------------
-- Yank to system clipboard
vim.keymap.set({ "n", "v" }, "<leader>y", '"+y', { desc = "Yank to system clipboard" })
vim.keymap.set({ "n", "v" }, "<leader>Y", '"+Y', { desc = "Yank to system clipboard (line)" })

-- Paste from system clipboard
vim.keymap.set({ "n", "v" }, "<leader>p", '"+p', { desc = "Paste from system clipboard" })

----------------------------------------
--- Copilot and completion integration
----------------------------------------
-- Smart <Tab>: Copilot if visible, else completion, else insert tab
vim.keymap.set("i", "<Tab>", function()
  local ok, s = pcall(require, "copilot.suggestion")
  if ok and s.is_visible() then
    s.accept()
    return ""
  end
  if vim.fn.pumvisible() == 1 then
    return "<C-n>"
  end
  return "\t"
end, { expr = true, silent = true })

-- Smart <S-Tab>: previous completion item, else backspace a tab stop
vim.keymap.set("i", "<S-Tab>", function()
  if vim.fn.pumvisible() == 1 then
    return "<C-p>"
  end
  return "<BS>"
end, { expr = true, silent = true })

----------------------------------------
--- Saving keymaps
----------------------------------------
-- Save with Ctrl+S in all modes
vim.keymap.set("n", "<C-s>", ":w<CR>", { noremap = true, silent = true, desc = "Save file" })
vim.keymap.set("i", "<C-s>", "<Esc>:w<CR>a", { noremap = true, silent = true, desc = "Save file (insert mode)" })
vim.keymap.set("v", "<C-s>", "<Esc>:w<CR>", { noremap = true, silent = true, desc = "Save file (visual mode)" })

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

---------- End of File ----------
