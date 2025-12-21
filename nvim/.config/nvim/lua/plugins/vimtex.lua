return {
  "lervag/vimtex",
  lazy = false, -- load immediately
  init = function()
    -- Use Zathura as PDF viewer
    vim.g.vimtex_view_method = "zathura"

    -- Compile automatically using latexmk
    vim.g.vimtex_compiler_method = "latexmk"

    -- Optional: prevent auto-opening quickfix window
    vim.g.vimtex_quickfix_mode = 0

    -- Optional: use a local build directory
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
