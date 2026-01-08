vim.keymap.set("n", "<leader>a", "<cmd>Assistant<cr>", { buffer = true, desc = "Assistant.nvim" })

-- Helper: Access the Vimscript function from the plugin
-- We wrap this to ensure we don't crash if the plugin isn't installed/active
local W = vim.fn["switch#Words"]

-- Define the exact-width integer cycles
local fixed_width_cycles = {
  W({ "int8_t", "int16_t", "int32_t", "int64_t", "__int128" }),
  W({ "uint8_t", "uint16_t", "uint32_t", "uint64_t", "__uint128_t" }),
  W({ "__int128", "__int256", "__int512", "__int1024" }),
  W({ "__uint128_t", "__uint256", "__uint512", "__uint1024" }),
}

-- Define the safe type promotion cycle
--    Note: We use standard strings with double backslashes here because
--    they contain complex regex groups that are easier to read this way.
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

-- Get existing buffer-local definitions (safely)
local current_defs = vim.b.switch_custom_definitions or {}

-- Merge our new rules
table.insert(current_defs, width_cycle_safe)

for _, cycle in ipairs(fixed_width_cycles) do
  table.insert(current_defs, cycle)
end

vim.b.switch_custom_definitions = current_defs
