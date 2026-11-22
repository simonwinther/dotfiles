-- ~/.config/nvim/lua/plugins/markdown.lua
local function show_cursor_line(_, windows)
  for _, win in ipairs(windows) do
    vim.wo[win].concealcursor = "c"
  end
end

return {
  {
    "OXY2DEV/markview.nvim",
    lazy = false, -- Markview handles filetype attachment itself.
    opts = {
      latex = { enable = false },
      markdown_inline = { escapes = { enable = false } },
      preview = {
        icon_provider = "internal",
        callbacks = {
          on_hybrid_enable = show_cursor_line,
          on_hybrid_disable = show_cursor_line,
          on_mode_change = function(buffer, windows, mode)
            local modes = require("markview.spec").get({ "preview", "modes" })
            for _, win in ipairs(windows) do
              vim.wo[win].conceallevel = vim.list_contains(modes, mode) and 3 or 0
            end
            show_cursor_line(buffer, windows)
          end,
        },
      },
    },
    config = function(_, opts)
      -- Preserve literal backslashes even while an unfinished equation is
      -- temporarily parsed as ordinary Markdown.
      local queries = {}
      for _, path in ipairs(vim.treesitter.query.get_files("markdown_inline", "highlights")) do
        local query = table.concat(vim.fn.readfile(path), "\n")
        query = query:gsub("%(%(backslash_escape%) @conceal%s+%b()%s+%b()%)", "")
        queries[#queries + 1] = query
      end
      if #queries > 0 then
        vim.treesitter.query.set("markdown_inline", "highlights", table.concat(queries, "\n"))
      end
      local presets = require("markview.presets") -- safe here
      opts.markdown = opts.markdown or {}
      opts.markdown.headings = presets.headings.slanted -- pick a preset
      require("markview").setup(opts)
    end,
  },
}
