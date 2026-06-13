local function cmdline_range_prefix_width()
  local range = vim.fn.getcmdline():match("^%s*'<%s*,%s*'>%s*") or vim.fn.getcmdline():match("^%s*%d+%s*,%s*%d+%s*")

  return range and vim.fn.strdisplaywidth(range) or 0
end

-- Absolute screen rectangle (0-indexed) of a floating window. Uses
-- win_screenpos so the coordinates are comparable across windows regardless of
-- what each one is positioned `relative` to. Returns nil for non-floats.
local function valid_win(win)
  return type(win) == "number" and vim.api.nvim_win_is_valid(win)
end

local function border_char_width(border, index)
  local char = border[index]
  if type(char) == "table" then
    char = char[1]
  end

  if char == nil or char == "" then
    return 0
  end

  return 1
end

local function border_dimensions(border)
  if type(border) == "string" then
    if border == "" or border == "none" then
      return { width = 0, height = 0 }
    end

    if border == "shadow" then
      return { width = 1, height = 1 }
    end

    return { width = 2, height = 2 }
  end

  if type(border) ~= "table" then
    return { width = 0, height = 0 }
  end

  return {
    width = border_char_width(border, 8) + border_char_width(border, 4),
    height = border_char_width(border, 2) + border_char_width(border, 6),
  }
end

local function border_width(border)
  return border_dimensions(border).width
end

local function win_rect(win)
  if not win or not vim.api.nvim_win_is_valid(win) then
    return nil
  end

  local ok, cfg = pcall(vim.api.nvim_win_get_config, win)
  if not ok or cfg.relative == nil or cfg.relative == "" then
    return nil
  end

  local pos = vim.fn.win_screenpos(win) -- { row, col }, 1-indexed; {0,0} if hidden
  if type(pos) ~= "table" or type(pos[1]) ~= "number" or pos[1] <= 0 then
    return nil
  end

  local border = border_dimensions(cfg.border)

  return {
    col = pos[2] - 1,
    row = pos[1] - 1,
    width = vim.api.nvim_win_get_width(win) + border.width,
    height = vim.api.nvim_win_get_height(win) + border.height,
  }
end

-- The visible cmdline box auto-grows with its content, and noice keeps the
-- real box border on the active Nui view. Reading that border window directly
-- keeps the edges tied to the expanded box instead of inferring them from the
-- command text width.
local function cmdline_box_rect()
  local ok, cmdline = pcall(require, "noice.ui.cmdline")
  if not ok or type(cmdline.win) ~= "function" then
    return nil
  end

  local content = cmdline.win()
  if not valid_win(content) then
    return nil
  end

  local router_ok, router = pcall(require, "noice.message.router")
  if router_ok and type(router.get_views) == "function" then
    for view in pairs(router.get_views()) do
      local nui = view._nui
      if nui ~= nil and nui.winid == content then
        local border = nui.border
        if border ~= nil and valid_win(border.winid) then
          return win_rect(border.winid)
        end

        return win_rect(content)
      end
    end
  end

  return win_rect(content)
end

-- Screen columns (0-indexed) of the cmdline box's left and right borders. The
-- right value is the column the menu's right border should land on to merge;
-- the left value is the lowest column the menu may be pulled to. Returns
-- nil, nil when the box window can't be read.
local function cmdline_box_edges()
  local box = cmdline_box_rect()
  if box == nil then
    return nil, nil
  end

  -- The border window occupies [col, col + width - 1]; its last column carries
  -- the right border, its first column the left border.
  local box_left = math.max(math.floor(box.col), 0)
  local box_right = math.floor(box.col + box.width - 1)

  return box_left, box_right
end

-- Border order: top-left, top, top-right, right, bottom-right, bottom,
-- bottom-left, left.
--
-- Left side: merges into the cmdline box (flush command name) or curves
-- (floating). A blank-but-non-empty (" ") top edge is required for Neovim to
-- render the top corners; with "" it drops the whole top row, so the merge
-- state uses "" deliberately to blend straight into the box above.
--
-- Right side: curves, unless the menu reaches the box's right edge, in which
-- case it becomes a vertical line continuing the box's right border downward.
local function cmdline_menu_border(merge_left, merge_right)
  return {
    merge_left and "" or "╮",
    merge_left and "" or " ",
    merge_right and "│" or "╭",
    "│",
    "╯",
    "─",
    "╰",
    "│",
  }
end

local function cmdline_reflow_delay()
  local ok, config = pcall(require, "noice.config")
  local throttle = ok and config.options and tonumber(config.options.throttle) or nil

  return throttle and math.ceil(throttle) or 0
end

local function update_cmdline_menu_position()
  if vim.api.nvim_get_mode().mode ~= "c" then
    return
  end

  local ok, menu = pcall(require, "blink.cmp.completion.windows.menu")
  if not ok or not menu.win or not menu.win:is_open() then
    return
  end

  pcall(menu.update_position)
end

local cmdline_menu_reflow_pending = false

local function schedule_cmdline_menu_reflow()
  if cmdline_menu_reflow_pending then
    return
  end

  cmdline_menu_reflow_pending = true
  vim.schedule(update_cmdline_menu_position)

  local delay = cmdline_reflow_delay()
  if delay > 0 then
    vim.defer_fn(function()
      cmdline_menu_reflow_pending = false
      update_cmdline_menu_position()
    end, delay)
  else
    vim.schedule(function()
      cmdline_menu_reflow_pending = false
    end)
  end
end

return {
  {
    "saghen/blink.cmp",
    dependencies = {
      "giuxtaposition/blink-cmp-copilot",
    },
    init = function()
      vim.api.nvim_create_autocmd("User", {
        group = vim.api.nvim_create_augroup("DotfilesBlinkCmdlineMenu", { clear = true }),
        pattern = "BlinkCmpShow",
        callback = schedule_cmdline_menu_reflow,
      })
    end,
    opts = {
      -- Use LuaSnip as the snippet engine
      snippets = { preset = "luasnip" },

      -- Define Sources
      sources = {
        default = { "lsp", "path", "snippets", "buffer", "copilot" },
        providers = {
          copilot = {
            name = "copilot",
            module = "blink-cmp-copilot",
            score_offset = 100,
            async = true,
            transform_items = function(_, items)
              local CompletionItemKind = require("blink.cmp.types").CompletionItemKind
              local kind_idx = CompletionItemKind.Copilot
              items = items or {}
              for _, item in ipairs(items) do
                item.kind = kind_idx
              end
              return items
            end,
          },
        },
      },

      -- Appearance & Icons
      appearance = {
        kind_icons = {
          Copilot = "",
        },
      },

      -- Menu Customization (Right column for source name)
      completion = {
        list = {
          selection = {
            preselect = true,
            auto_insert = false,
          },
        },
        -- Disable ghost text (prevents clash with Copilot's ghost text)
        ghost_text = { enabled = false },
        documentation = {
          window = {
            border = "rounded",
            max_width = 100,
            max_height = 12,
            scrollbar = false,
          },
        },
        menu = {
          border = "rounded",
          scrollbar = false,
          cmdline_position = function()
            if vim.g.ui_cmdline_pos ~= nil then
              local pos = vim.g.ui_cmdline_pos
              local col = math.max(pos[2] - cmdline_range_prefix_width(), 0)

              local ok, menu = pcall(require, "blink.cmp.completion.windows.menu")
              if ok and menu.win and menu.win:is_open() then
                local win = menu.win:get_win()

                local ctx = menu.context
                local start_col = ctx ~= nil and ctx.bounds ~= nil and ctx.bounds.start_col or nil
                if type(start_col) ~= "number" then
                  start_col = nil
                end
                local keyword_offset = start_col and (start_col - 1 - cmdline_range_prefix_width()) or nil

                local align_ok, align = pcall(function()
                  return menu.renderer:get_alignment_start_col()
                end)
                if not align_ok or type(align) ~= "number" then
                  align = 0
                end

                local merge_left = keyword_offset == nil or keyword_offset <= 0
                local merge_right = false
                local border = cmdline_menu_border(merge_left, merge_right)

                if start_col ~= nil then
                  local box_left, box_right = cmdline_box_edges()
                  local content_width = vim.api.nvim_win_get_width(win)
                  local outer_width = content_width + border_width(border)
                  local left = math.max(col + start_col - align, 0)

                  if box_left ~= nil and left <= box_left then
                    left = box_left
                    merge_left = true
                  elseif box_left ~= nil then
                    merge_left = false
                  end

                  border = cmdline_menu_border(merge_left, merge_right)
                  col = left - start_col + align

                  if box_left ~= nil and box_right ~= nil and left + outer_width - 1 >= box_right then
                    merge_right = true
                    border = cmdline_menu_border(merge_left, merge_right)

                    local box_width = box_right - box_left + 1
                    local max_content_width = box_width - border_width(border)
                    if max_content_width > 0 and content_width > max_content_width then
                      if pcall(vim.api.nvim_win_set_width, win, max_content_width) then
                        content_width = max_content_width
                      end
                    end

                    outer_width = content_width + border_width(border)
                    left = math.max(box_right - outer_width + 1, box_left)
                    if left <= box_left then
                      merge_left = true
                      border = cmdline_menu_border(merge_left, merge_right)
                    end

                    col = left - start_col + align
                  end
                end

                pcall(vim.api.nvim_win_set_config, win, {
                  border = border,
                })
              end

              return { pos[1] - 1, col }
            end

            local height = (vim.o.cmdheight == 0) and 1 or vim.o.cmdheight
            return { vim.o.lines - height, 0 }
          end,
          draw = {
            columns = { { "kind_icon" }, { "label", "label_description", gap = 1 }, { "source_name" } },
            components = {
              source_name = {
                text = function(ctx)
                  return "[" .. ctx.source_name .. "]"
                end,
                highlight = "BlinkCmpSource",
              },
            },
          },
        },
      },

      signature = {
        window = {
          border = "rounded",
          max_width = 100,
          max_height = 12,
          scrollbar = false,
        },
      },

      -- Keymaps
      keymap = {
        preset = "default",
        ["<C-k>"] = { "show_signature", "hide_signature", "fallback" },
        ["<Tab>"] = {
          function(cmp)
            local ok, suggestion = pcall(require, "copilot.suggestion")
            if ok and suggestion.is_visible() then
              suggestion.accept()
              return true
            end

            if cmp.is_menu_visible() then
              return cmp.select_and_accept()
            end
          end,
          "fallback",
        },
        ["<S-Tab>"] = {
          function()
            return vim.api.nvim_replace_termcodes("<C-d>", true, true, true)
          end,
        },
      },
    },
  },
}
