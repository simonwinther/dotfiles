return {
  "stevearc/conform.nvim",
  opts = {
    formatters = {
      clang_format = {
        command = "clang-format",
      },
    },
    formatters_by_ft = {
      c = { "clang_format" },
      cpp = { "clang_format" },
      python = { "isort", "black" },
    },
  },
}
