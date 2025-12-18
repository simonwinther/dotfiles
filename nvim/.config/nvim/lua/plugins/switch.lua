return {
  "AndrewRadev/switch.vim",
  config = function()
    local S = vim.fn["switch#NormalizedCaseWords"]

    ---------------------------------------------------------------------------
    -- 🌍 Global definitions (apply to all filetypes)
    ---------------------------------------------------------------------------
    vim.g.switch_custom_definitions = {
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

    ---------------------------------------------------------------------------
    -- 🖥 C / C++ specific definitions
    ---------------------------------------------------------------------------

    vim.api.nvim_create_autocmd("FileType", {
      pattern = { "cpp" },
      callback = function()
        local W = vim.fn["switch#Words"]

        -- Exact width and big-int cycles
        local fixed_width_cycles = {
          W({ "int8_t", "int16_t", "int32_t", "int64_t", "__int128" }),
          W({ "uint8_t", "uint16_t", "uint32_t", "uint64_t", "__uint128_t" }),
          W({ "__int128", "__int256", "__int512", "__int1024" }),
          W({ "__uint128_t", "__uint256", "__uint512", "__uint1024" }),
        }

        -- char -> short -> int -> long -> long long -> char
        local width_cycle_safe = {
          ["\\<char\\>"] = "short",
          ["\\<short\\>"] = "int",
          ["\\<int\\>"] = "long",

          -- upgrade 'long' only if it's NOT already 'long long'
          -- (\%( \+long)\@! is a negative lookahead for " space+long")
          ["\\<long\\>\\%( \\+long\\)\\@!"] = "long long",

          -- wrap 'long long' back to 'char'
          ["\\<long\\>\\s\\+\\<long\\>"] = "char",
        }

        -- Merge everything without clobbering other cpp rules you might add
        local defs = vim.b.switch_custom_definitions or {}
        table.insert(defs, width_cycle_safe)
        for _, d in ipairs(fixed_width_cycles) do
          table.insert(defs, d)
        end
        vim.b.switch_custom_definitions = defs
      end,
    })
  end,
}
