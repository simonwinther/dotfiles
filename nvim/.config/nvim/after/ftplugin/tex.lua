-- ~/.config/nvim/after/ftplugin/tex.lua
-- LaTeX Specific Configuration
vim.opt_local.wrap = true
vim.opt_local.linebreak = true
vim.opt_local.breakindent = true

--------------------------------------------------------------------------------
-- KEYMAPS
--------------------------------------------------------------------------------
local function preview_latex_image()
  local ok, image = pcall(require, "snacks.image")
  if not ok then
    vim.notify("snacks.nvim image preview is unavailable", vim.log.levels.WARN)
    return
  end

  image.hover()
end

local function close_latex_image_preview()
  local ok, image_doc = pcall(require, "snacks.image.doc")
  if ok then
    image_doc.hover_close()
  end
end

-- Paste Image (img-clip.nvim)
-- Use require
vim.keymap.set("n", "\\lp", function()
  require("img-clip").paste_image()
end, {
  desc = "[img-clip.nvim] Paste image from system clipboard",
  buffer = true,
})

vim.keymap.set("n", "\\li", preview_latex_image, {
  desc = "[snacks.nvim] Preview image/formula under cursor",
  buffer = true,
})

local current_buf = vim.api.nvim_get_current_buf()
local image_preview_group = vim.api.nvim_create_augroup("tex_snacks_image_preview_" .. current_buf, {
  clear = true,
})

vim.api.nvim_create_autocmd("CursorHold", {
  group = image_preview_group,
  buffer = current_buf,
  callback = preview_latex_image,
})

vim.api.nvim_create_autocmd({ "CursorMoved", "InsertEnter", "BufLeave" }, {
  group = image_preview_group,
  buffer = current_buf,
  callback = function()
    vim.schedule(close_latex_image_preview)
  end,
})

--------------------------------------------------------------------------------
-- NVIM-SURROUND SETUP
--------------------------------------------------------------------------------
-- Use pcall to safely check if nvim-surround is installed
local surround_available, ns = pcall(require, "nvim-surround")

if surround_available then
  ns.buffer_setup({
    aliases = { ["b"] = false }, -- Disable default 'b' alias to use bold
    surrounds = {
      e = { add = { "\\emph{", "}" } },
      b = { add = { "\\textbf{", "}" } },
      ["}"] = { add = { "\\underbrace{", "}_{}" } },
      ["{"] = { add = { "\\overbrace{", "}^{}" } },
      i = { add = { "\\textit{", "}" } },
      u = { add = { "\\underline{", "}" } },
      c = { add = { "\\cite{", "}" } },
      t = { add = { "\\texttt{", "}" } },
      m = { add = { "\\(", "\\)" } }, -- Inline math
      d = { add = { "\\[", "\\]" } }, -- Display math
    },
  })
end

--------------------------------------------------------------------------------
-- SWITCH.NVIM CONFIGURATION
--------------------------------------------------------------------------------
-- Define custom toggle pairs for LaTeX
local latex_switches = {
  { "mathcal", "mathbb", "mathfrak", "mathbf", "mathrm", "mathsf", "mathtt" },
  { [[\\begin{itemize}]], [[\\begin{enumerate}]], [[\\begin{description}]] },
  { [[\\end{itemize}]], [[\\end{enumerate}]], [[\\end{description}]] },

  { [[\\section]], [[\\subsection]], [[\\subsubsection]] },
  { [[\\section*]], [[\\subsection*]], [[\\subsubsection*]] },

  { [[\\begin{equation}]], [[\\begin{align}]], [[\\begin{gather}]] },
  { [[\\end{equation}]], [[\\end{align}]], [[\\end{gather}]] },

  { [[\\begin{equation*}]], [[\\begin{align*}]], [[\\begin{gather*}]] },
  { [[\\end{equation*}]], [[\\end{align*}]], [[\\end{gather*}]] },
}

local global_defs = vim.g.switch_custom_definitions or {}

local master_list = {}

-- Insert LaTeX rules first (High Priority)
for _, def in ipairs(latex_switches) do
  table.insert(master_list, def)
end

-- Insert Global rules second (Low Priority)
for _, def in ipairs(global_defs) do
  table.insert(master_list, def)
end

-- Apply to the buffer
vim.b.switch_custom_definitions = master_list
