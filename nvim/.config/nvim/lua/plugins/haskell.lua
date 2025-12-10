return {
  -- Simon, haskell-tools er hls, så vi skal slå hls client fra nvim-lspconfig,
  -- siden haskell-tools.nvim allerede starter HLS, ellers ender vi med 2 af alt ... xD
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        hls = false,
      },
    },
  },
}
