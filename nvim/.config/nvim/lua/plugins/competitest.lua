return {
  "xeluxee/competitest.nvim",
  dependencies = {
    "MunifTanjim/nui.nvim",
  },
  cmd = "CompetiTest",
  config = function()
    require("competitest").setup({
      local_config_file_name = ".competitest.lua", -- Looks for this file in the current working directory and loads it if found.
    })
  end,
}
