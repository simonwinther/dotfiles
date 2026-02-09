-- ~/.config/nvim/after/ftplugin/tex.lua
-- LaTeX Specific Configuration

--------------------------------------------------------------------------------
-- KEYMAPS
--------------------------------------------------------------------------------
-- Paste Image (img-clip.nvim)
-- Use require
vim.keymap.set("n", "\\lp", function()
  require("img-clip").paste_image()
end, {
  desc = "[img-clip.nvim] Paste image from system clipboard",
  buffer = true,
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
