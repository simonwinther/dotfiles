return {
  "lervag/vimtex",
  lazy = true, -- override
  ft = { "tex", "plaintex" },
  init = function()
    -- Use Zathura as PDF viewer
    vim.g.vimtex_view_method = "zathura"
    vim.g.vimtex_compiler_method = "latexmk"

    -- prevent auto-opening quickfix window
    vim.g.vimtex_quickfix_mode = 0

    vim.g.vimtex_compiler_latexmk = {
      build_dir = "build",
      callback = 1,
      continuous = 1,
      executable = "latexmk",
      options = {
        "-pdf",
        "-interaction=nonstopmode",
        "-synctex=1",
      },
      clean = 1,
    }
  end,
}
