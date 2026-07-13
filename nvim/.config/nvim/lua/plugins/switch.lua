local function modifier(kind, definition)
  return { _type = kind, _definition = definition }
end

local function words(definition)
  return modifier("words", definition)
end

local function normalized_case_words(definition)
  return modifier("normalized_case_words", definition)
end

local function cycle_words(definition)
  local indices = {}
  for index, value in ipairs(definition) do
    indices[value] = index
  end

  local pattern = ([[\C\<\(%s\)\>]]):format(table.concat(definition, [[\|]]))

  return {
    [pattern] = function(matches, reverse, count)
      local value = matches[1]
      local index = indices[value]
      if not index then
        return value
      end

      local steps = tonumber(count) or 0
      steps = steps == 0 and 1 or steps

      local direction = reverse == 1 and -1 or 1
      return definition[((index - 1 + direction * steps) % #definition) + 1]
    end,
  }
end

local nested_braces = [[\%([^{}]\|{[^{}]*}\)*]]
local underbrace_pattern = [[\\underbrace{\(]] .. nested_braces .. [[\)}_{\(]] .. nested_braces .. [[\)}]]
local overbrace_pattern = [[\\overbrace{\(]] .. nested_braces .. [[\)}\^{\(]] .. nested_braces .. [[\)}]]

local latex_definitions = {
  { "mathcal", "mathbb", "mathfrak", "mathbf", "mathrm", "mathsf", "mathtt" },
  { [[\\begin{itemize}]], [[\\begin{enumerate}]], [[\\begin{description}]] },
  { [[\\end{itemize}]], [[\\end{enumerate}]], [[\\end{description}]] },
  { [[\\section]], [[\\subsection]], [[\\subsubsection]] },
  { [[\\section*]], [[\\subsection*]], [[\\subsubsection*]] },
  { [[\\begin{equation}]], [[\\begin{align}]], [[\\begin{gather}]] },
  { [[\\end{equation}]], [[\\end{align}]], [[\\end{gather}]] },
  { [[\\begin{equation*}]], [[\\begin{align*}]], [[\\begin{gather*}]] },
  { [[\\end{equation*}]], [[\\end{align*}]], [[\\end{gather*}]] },
  {
    [underbrace_pattern] = [[\\overbrace{\1}^{\2}]],
    [overbrace_pattern] = [[\\underbrace{\1}_{\2}]],
  },
}

local c_family_definitions = {
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

local definitions_by_filetype = {
  c = c_family_definitions,
  cpp = c_family_definitions,
  tex = latex_definitions,
}

local global_definitions = {
  cycle_words({ "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat" }),
  cycle_words({ "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday" }),
  cycle_words({ "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" }),
  cycle_words({
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
  }),

  -- Boolean and flag switches
  normalized_case_words({ "true", "false" }),
  normalized_case_words({ "on", "off" }),
  normalized_case_words({ "yes", "no" }),
  words({ "T", "F" }),
  { "define", "undef" },

  -- Visibility and access modifiers
  normalized_case_words({ "public", "protected", "private" }),

  -- Dimensions and directions
  { "width", "height" },
  { "top", "bottom" },
  { "left", "right" },
  { "up", "down" },
  { "row", "column" },
  { "horizontal", "vertical" },

  -- Control flow and testing
  { "break", "continue" },
  { "assert", "refute" },
  { "expected", "actual" },

  -- Comparison and assignment
  { "!==", "===" },
  {
    ["\\%(^\\|[^!=]\\)\\zs==\\ze\\%($\\|[^=]\\)"] = "!=",
    ["\\%(^\\|[^!]\\)\\zs!=\\ze\\%($\\|[^=]\\)"] = "==",
  },
  { ">=", "<=" },
  { "+=", "-=" },
  { "*=", "/=" },

  -- Arithmetic
  { "++", "--" },
  {
    ["\\%(^\\|[^+]\\)\\zs[+]\\ze\\%($\\|[^+]\\)"] = "-",
    ["\\%(^\\|[^-]\\)\\zs[-]\\ze\\%($\\|[^-]\\)"] = "+",
  },
  { "*", "/" },
  { ">", "<" },
  { "min", "max" },
  { "floor", "ceil" },
  { "abs", "-abs" },

  -- Logical and bitwise operators
  { "&&", "||" },
  {
    ["\\%(^\\|[^|]\\)\\zs[|]\\ze\\%($\\|[^|]\\)"] = "&",
    ["\\%(^\\|[^&]\\)\\zs[&]\\ze\\%($\\|[^&]\\)"] = "|",
  },

  -- Word-only string delimiters
  {
    ['"\\(\\k\\+\\)"'] = [[`\1`]],
    ["`\\(\\k\\+\\)`"] = [=['\1']=],
    ["'\\(\\k\\+\\)'"] = [["\1"]],
  },
}

local function configure_definitions()
  vim.g.switch_mapping = ""
  vim.g.switch_reverse_mapping = ""
  vim.g.switch_custom_definitions = global_definitions

  local group = vim.api.nvim_create_augroup("DotfilesSwitchDefinitions", { clear = true })
  vim.api.nvim_create_autocmd("FileType", {
    group = group,
    pattern = { "c", "cpp", "tex" },
    callback = function(event)
      vim.b[event.buf].switch_custom_definitions = definitions_by_filetype[event.match]
    end,
  })
end

local binary_edge_cases = {
  [false] = { from = "0B11", to = "0B101" },
  [true] = { from = "0b100", to = "0b010" },
}

local function replace_binary_edge_case(reverse, count)
  if count > 1 then
    return false
  end

  local line = vim.api.nvim_get_current_line()
  local cursor = vim.api.nvim_win_get_cursor(0)
  local cursor_column = cursor[2] + 1
  local search_start = 1
  local edge_case = binary_edge_cases[reverse]

  while true do
    local token_start, token_end = line:find("0[bB][01]+", search_start)
    if not token_start then
      return false
    end

    if token_start <= cursor_column and cursor_column <= token_end then
      if line:sub(token_start, token_end) ~= edge_case.from then
        return false
      end

      vim.api.nvim_buf_set_text(0, cursor[1] - 1, token_start - 1, cursor[1] - 1, token_end, { edge_case.to })
      return true
    end

    search_start = token_end + 1
  end
end

local function switch_or_increment(reverse)
  local count = tonumber(vim.v.count) or 0
  local count1 = count == 0 and 1 or count

  if replace_binary_edge_case(reverse, count) then
    return
  end

  local options = { count = count }
  if reverse then
    options.reverse = 1
  end

  if vim.fn["switch#Switch"](options) == 1 then
    return
  end

  local fallback = vim.keycode(reverse and "<C-x>" or "<C-a>")
  vim.cmd.normal({ args = { count1 .. fallback }, bang = true })
end

return {
  "AndrewRadev/switch.vim",
  keys = {
    {
      "gs",
      function()
        switch_or_increment(false)
      end,
      desc = "Increment or switch forward",
    },
    {
      "GS",
      function()
        switch_or_increment(true)
      end,
      desc = "Decrement or switch reverse",
    },
  },
  init = configure_definitions,
}
