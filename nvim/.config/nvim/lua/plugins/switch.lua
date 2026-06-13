return {
  "AndrewRadev/switch.vim",
  keys = {
    { "gs", "<Plug>(Switch)", desc = "Switch case/bool/item" },
    { "GS", "<Plug>(SwitchReverse)", desc = "Switch item reverse" },
  },

  -- init() runs at startup, before the plugin is lazy-loaded. The FileType
  -- autocmds below only assign the buffer-local b:switch_custom_definitions
  -- variable, which switch.vim reads lazily when `gs` is pressed. So opening a
  -- tex/c/cpp buffer costs a single table assignment and does NOT load the
  -- plugin, and unrelated filetypes (java, python, ...) get no buffer-local
  -- definitions at all. This is switch.vim's documented pattern for
  -- filetype-specific definitions (:h b:switch_custom_definitions).
  init = function()
    -- Thin wrapper matching switch#Words(); inlined so the definition tables
    -- can be built without the plugin being loaded yet.
    local function words(def)
      return { _type = "words", _definition = def }
    end

    -------------------------------------------------------------------------
    -- LaTeX
    -------------------------------------------------------------------------
    -- Allow one level of nested braces in the captured body/label via
    -- \%([^{}]\|{[^{}]*}\)* so e.g. \underbrace{x_{1}}_{\text{sum}} matches.
    -- A plain [^{}]* only matches brace-free content and silently fails on
    -- the common case of subscripts or \text{} labels.
    local brace = [[\%([^{}]\|{[^{}]*}\)*]]
    local latex_switches = {
      { "mathcal", "mathbb", "mathfrak", "mathbf", "mathrm", "mathsf", "mathtt" },
      { [[\\begin{itemize}]], [[\\begin{enumerate}]], [[\\begin{description}]] },
      { [[\\end{itemize}]], [[\\end{enumerate}]], [[\\end{description}]] },

      { [[\\section]], [[\\subsection]], [[\\subsubsection]] },
      { [[\\section*]], [[\\subsection*]], [[\\subsubsection*]] },

      { [[\\begin{equation}]], [[\\begin{align}]], [[\\begin{gather}]] },
      { [[\\end{equation}]], [[\\end{align}]], [[\\end{gather}]] },

      { [[\\begin{equation*}]], [[\\begin{align*}]], [[\\begin{gather*}]] },
      { [[\\end{equation*}]], [[\\end{align*}]], [[\\end{gather*}]] },

      -- Underbrace | Overbrace
      {
        [ [[\\underbrace{\(]] .. brace .. [[\)}_{\(]] .. brace .. [[\)}]] ] = [[\\overbrace{\1}^{\2}]],
        [ [[\\overbrace{\(]] .. brace .. [[\)}\^{\(]] .. brace .. [[\)}]] ] = [[\\underbrace{\1}_{\2}]],
      },
    }

    vim.api.nvim_create_autocmd("FileType", {
      pattern = "tex",
      callback = function()
        vim.b.switch_custom_definitions = latex_switches
      end,
    })

    -------------------------------------------------------------------------
    -- C / C++
    -------------------------------------------------------------------------
    local cpp_switches = {
      words({ "int8_t", "int16_t", "int32_t", "int64_t", "__int128" }),
      words({ "uint8_t", "uint16_t", "uint32_t", "uint64_t", "__uint128_t" }),
      words({ "__int128", "__int256", "__int512", "__int1024" }),
      words({ "__uint128_t", "__uint256", "__uint512", "__uint1024" }),

      {
        ["\\<char\\>"] = "short",
        ["\\<short\\>"] = "int",
        ["\\<int\\>"] = "long",
        ["\\<long\\>\\s\\+\\<long\\>"] = "float",
        ["\\<long\\>"] = "long long",
        ["\\<float\\>"] = "double",
        ["\\<double\\>"] = "char",
      },
    }

    vim.api.nvim_create_autocmd("FileType", {
      pattern = { "c", "cpp" },
      callback = function()
        vim.b.switch_custom_definitions = cpp_switches
      end,
    })
  end,

  config = function()
    local S = vim.fn["switch#NormalizedCaseWords"]

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
    }
  end,
}
