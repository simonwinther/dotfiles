-- ~/.config/nvim/after/ftplugin/tex.lua
-- LaTeX Specific Configuration

--------------------------------------------------------------------------------
-- KEYMAPS
--------------------------------------------------------------------------------
-- Paste Image (img-clip.nvim)
vim.keymap.set("n", "\\lp", "<cmd>PasteImage<cr>", {
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
  -- Environments
  { [[\\begin{itemize}]], [[\\begin{enumerate}]], [[\\begin{description}]] },
  { [[\\end{itemize}]], [[\\end{enumerate}]], [[\\end{description}]] },

  -- Sections
  { [[\\section]], [[\\subsection]], [[\\subsubsection]] },

  -- Math Environments
  { [[\\begin{equation}]], [[\\begin{align}]], [[\\begin{gather}]] },
  { [[\\end{equation}]], [[\\end{align}]], [[\\end{gather}]] },
}

-- Merge with existing switches safely
local current_defs = vim.b.switch_custom_definitions or {}

for _, def in ipairs(latex_switches) do
  table.insert(current_defs, def)
end

vim.b.switch_custom_definitions = current_defs
