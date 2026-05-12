return {
  "abecodes/tabout.nvim",
  event = "InsertCharPre", -- Load right before inserting a character, so it does not load during startup/navigation
  dependencies = {
    "nvim-treesitter/nvim-treesitter",
    "L3MON4D3/LuaSnip",
    -- nvim-cmp removed because this config uses blink.cmp instead
  },
  opts = {
    tabkey = "<A-l>", -- Key to jump out of pairs/brackets/quotes
    backwards_tabkey = "<A-h>", -- Key to jump backwards out of pairs/brackets/quotes
    act_as_tab = false, -- Do not shift content if tabout is not possible
    act_as_shift_tab = false, -- Do not reverse-shift content if backwards tabout is not possible
    default_tab = "<C-t>", -- Fallback action for normal tab behavior at the beginning of a line
    default_shift_tab = "<C-d>", -- Fallback action for reverse tab behavior
    enable_backwards = true, -- Enable backwards tabout
    completion = false, -- Do not integrate with completion popup handling
    tabouts = {
      { open = "'", close = "'" },
      { open = '"', close = '"' },
      { open = "`", close = "`" },
      { open = "(", close = ")" },
      { open = "[", close = "]" },
      { open = "{", close = "}" },
    },
    ignore_beginning = true, -- If cursor is at the beginning of a filled element, prefer tabout instead of shifting
    exclude = {}, -- Filetypes where tabout should be disabled
  },
}
