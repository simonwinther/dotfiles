return {
  "AndrewRadev/switch.vim",
  keys = {
    { "gs", "<Plug>(Switch)", desc = "Switch case/bool/item" },
    { "GS", "<Plug>(SwitchReverse)", desc = "Switch item reverse" },
  },
  config = function()
    local S = vim.fn["switch#NormalizedCaseWords"]
    local W = vim.fn["switch#Words"]

    local danish_lower = vim.fn.split("abcdefghijklmnopqrstuvwxyzæøå", "\\zs")
    local danish_upper = vim.fn.split("ABCDEFGHIJKLMNOPQRSTUVWXYZÆØÅ", "\\zs")

    ---------------------------------------------------------------------------
    -- Global definitions
    ---------------------------------------------------------------------------
    vim.g.switch_custom_definitions = {
      -- Boolean / flag switches
      S({ "true", "false" }),
      S({ "on", "off" }),
      S({ "yes", "no" }),

      -- numeric / simple toggles
      { "0", "1" },
      { "define", "undef" },

      -- Visibility / Access Modifiers
      S({ "public", "protected", "private" }),

      -- Dimensions & Directions (CSS, UI, Game Dev)
      { "width", "height" },
      { "top", "bottom" },
      { "left", "right" },
      { "up", "down" },
      { "row", "column" },
      { "horizontal", "vertical" },

      -- Loop Control
      { "break", "continue" },

      -- Testing / Assertions
      { "assert", "refute" },
      { "expected", "actual" },

      -- comparison and assignment
      { "==", "!=" },
      { ">=", "<=" },
      { "+=", "-=" },
      { "*=", "/=" },

      -- increment / decrement
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

      -- danish_lower,
      -- danish_upper,
    }

    ---------------------------------------------------------------------------
    -- Filetype Specific definitions (C / C++)
    ---------------------------------------------------------------------------
    local function setup_cpp_switch()
      if vim.b.switch_cpp_loaded then
        return
      end
      vim.b.switch_cpp_loaded = true

      local cpp_defs = {
        W({ "int8_t", "int16_t", "int32_t", "int64_t", "__int128" }),
        W({ "uint8_t", "uint16_t", "uint32_t", "uint64_t", "__uint128_t" }),
        W({ "__int128", "__int256", "__int512", "__int1024" }),
        W({ "__uint128_t", "__uint256", "__uint512", "__uint1024" }),
        {
          ["\\<char\\>"] = "short",
          ["\\<short\\>"] = "int",
          ["\\<int\\>"] = "long",
          ["\\<long\\>\\%( \\+long\\)\\@!"] = "long long",
          ["\\<long\\>\\s\\+\\<long\\>"] = "char",
        },
      }

      local current = vim.b.switch_custom_definitions or {}
      for _, def in ipairs(cpp_defs) do
        table.insert(current, def)
      end
      vim.b.switch_custom_definitions = current
    end

    -- Apply immediately if the buffer that triggered the lazy-load is C/C++
    if vim.bo.filetype == "cpp" or vim.bo.filetype == "c" then
      setup_cpp_switch()
    end

    -- Create an autocommand for any new C/C++ files opened later in the session
    vim.api.nvim_create_autocmd("FileType", {
      pattern = { "cpp", "c" },
      callback = setup_cpp_switch,
    })
  end,
}
