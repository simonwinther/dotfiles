return {
  "AndrewRadev/switch.vim",
  keys = {
    { "gs", "<Plug>(Switch)", desc = "Switch case/bool/item" },
    { "GS", "<Plug>(SwitchReverse)", desc = "Switch item reverse" },
  },
  config = function()
    local S = vim.fn["switch#NormalizedCaseWords"]

    local danish_lower = vim.fn.split("abcdefghijklmnopqrstuvwxyzæøå", "\\zs")
    local danish_upper = vim.fn.split("ABCDEFGHIJKLMNOPQRSTUVWXYZÆØÅ", "\\zs")
    ---------------------------------------------------------------------------
    -- 🌍 Global definitions (apply to all filetypes)
    ---------------------------------------------------------------------------
    vim.g.switch_custom_definitions = {
      -- Danish alphabet
      danish_lower,
      danish_upper,
      -- Boolean / flag switches (case-aware)
      S({ "true", "false" }),
      S({ "on", "off" }),
      S({ "yes", "no" }),

      -- numeric / simple toggles
      { "0", "1" },
      { "define", "undef" },

      -- 1. Visibility / Access Modifiers (Crucial for OOP)
      S({ "public", "protected", "private" }),

      -- 2. Dimensions & Directions (CSS, UI, Game Dev)
      { "width", "height" },
      { "top", "bottom" },
      { "left", "right" },
      { "up", "down" },
      { "row", "column" },
      { "horizontal", "vertical" },

      -- 3. Loop Control
      { "break", "continue" },

      -- 4. Testing / Assertions (Common in unit tests)
      { "assert", "refute" },
      { "expected", "actual" },

      -- comparison and assignment
      { "==", "!=" },
      { ">=", "<=" },
      { "+=", "-=" },
      { "*=", "/=" },

      -- increment / decrement (multi-char operators)
      { "++", "--" },
      { "--", "++" },

      -- SINGLE + / - only (never match ++ or --)
      {
        ["\\%(^\\|[^+]\\)\\zs[+]\\ze\\%($\\|[^+]\\)"] = "-",
        ["\\%(^\\|[^-]\\)\\zs[-]\\ze\\%($\\|[^-]\\)"] = "+",
      },

      -- arithmetic and relational
      { "*", "/" },
      { ">", "<" },
      { "min", "max" },
      { "floor", "ceil" },
      { "ceil", "floor" },
      { "abs", "-abs" },

      -- logical / bitwise
      { "&&", "||" },
      { "||", "&&" },
      -- Single | or & only (never match || or &&)
      {
        ["\\%(^\\|[^|]\\)\\zs[|]\\ze\\%($\\|[^|]\\)"] = "&",
        ["\\%(^\\|[^&]\\)\\zs[&]\\ze\\%($\\|[^&]\\)"] = "|",
      },
      -- generic string delimiters (word-only)
      {
        ['"\\(\\k\\+\\)"'] = [[`\1`]],
        ["`\\(\\k\\+\\)`"] = [=['\1']=],
        ["'\\(\\k\\+\\)'"] = [["\1"]],
      },
    }
  end,
}
