return {
  -- haskell-tools.nvim starts HLS itself; disable the lspconfig hls client to
  -- avoid duplicate clients.
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        hls = false,
      },
    },
  },
  {
    -- LazyVim maps this key to `:HlsEvalAll`, which haskell-tools does not
    -- define; call the evaluation API behind `:Haskell hls evalAll` instead.
    "mrcjkb/haskell-tools.nvim",
    keys = {
      {
        "<localleader>e",
        function()
          require("haskell-tools").lsp.buf_eval_all()
        end,
        ft = "haskell",
        desc = "Evaluate All",
      },
    },
  },
}
