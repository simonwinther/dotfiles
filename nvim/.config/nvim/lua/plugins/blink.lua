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

local function border_padding_dimensions(border)
  if type(border) ~= "table" or type(border.padding) ~= "table" then
    return { width = 0, height = 0 }
  end

  local padding = border.padding
  local top = padding.top or padding[1] or 0
  local right = padding.right or padding[2] or top
  local bottom = padding.bottom or padding[3] or top
  local left = padding.left or padding[4] or right

  return {
    width = left + right,
    height = top + bottom,
  }
end

local function noice_border_dimensions(border)
  local style = type(border) == "table" and border.style or border
  local padding = border_padding_dimensions(border)
  local chars = border_dimensions(style)

  return {
    width = chars.width + padding.width,
    height = chars.height + padding.height,
  }
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

local function editor_position_col(col, width)
  if type(col) == "number" then
    if col < 0 then
      return vim.o.columns + col - width
    end

    return col
  end

  if type(col) ~= "string" then
    return nil
  end

  local percent = col:match("^%s*([%d.]+)%%%s*$")
  if percent ~= nil then
    return math.floor((vim.o.columns - width) * tonumber(percent) / 100)
  end

  local number = tonumber(col)
  return number and editor_position_col(number, width) or nil
end

local function expected_cmdline_box_rect()
  local ok, cmdline = pcall(require, "noice.ui.cmdline")
  if not ok or type(cmdline.last) ~= "function" then
    return nil
  end

  local active = cmdline.last()
  if active == nil or type(active.state) ~= "table" then
    return nil
  end

  local current = setmetatable({
    state = vim.deepcopy(active.state),
    offset = active.offset,
  }, getmetatable(active))

  current.state.content = { { 0, vim.fn.getcmdline() } }
  current.state.pos = vim.fn.getcmdpos()

  if type(current.get_format) ~= "function" or type(current.format) ~= "function" then
    return nil
  end

  local format = current:get_format()
  local view = format and format.view or "cmdline_popup"

  local message_ok, Message = pcall(require, "noice.message")
  local views_ok, views = pcall(require, "noice.config.views")
  local nui_ok, nui = pcall(require, "noice.util.nui")
  if not message_ok or not views_ok or not nui_ok or type(views.get_options) ~= "function" then
    return nil
  end

  local message = Message("cmdline", nil)
  current:format(message)

  -- views.get_options returns the raw view config, which only carries `backend`.
  -- noice.util.nui.get_layout runs the options through `normalize`, which errors
  -- unless the resolved nui `type` is present (the popup/split backends set it
  -- at view-init time). Mirror that mapping here so get_layout never throws.
  local opts = vim.deepcopy(views.get_options(view))
  if opts.type == nil then
    local backend = type(opts.backend) == "table" and opts.backend[1] or opts.backend
    opts.type = backend == "split" and "split" or "popup"
  end

  local layout = nui.get_layout({ width = message:width(), height = message:height() }, opts)
  if
    layout == nil
    or type(layout.size) ~= "table"
    or type(layout.size.width) ~= "number"
    or type(layout.position) ~= "table"
  then
    return nil
  end

  local content_col = editor_position_col(layout.position.col, layout.size.width)
  if content_col == nil then
    return nil
  end

  local border = noice_border_dimensions(opts.border)
  local box_left = content_col - math.floor(border.width / 2 + 0.5)

  return {
    col = box_left,
    row = type(layout.position.row) == "number" and layout.position.row or 0,
    width = layout.size.width + border.width,
    height = (type(layout.size.height) == "number" and layout.size.height or 1) + border.height,
  }
end

local function merge_rects(a, b)
  if a == nil then
    return b
  end
  if b == nil then
    return a
  end

  local left = math.min(a.col, b.col)
  local top = math.min(a.row, b.row)
  local right = math.max(a.col + a.width - 1, b.col + b.width - 1)
  local bottom = math.max(a.row + a.height - 1, b.row + b.height - 1)

  return {
    col = left,
    row = top,
    width = right - left + 1,
    height = bottom - top + 1,
  }
end

-- The visible cmdline box auto-grows with its content, and noice keeps the
-- real box border on the active Nui view. Reading that border window directly
-- keeps the edges tied to the expanded box instead of inferring them from the
-- command text width.
local function cmdline_box_rect(use_expected)
  local live
  local ok, cmdline = pcall(require, "noice.ui.cmdline")
  if not ok or type(cmdline.win) ~= "function" then
    return use_expected and expected_cmdline_box_rect() or nil
  end

  local content = cmdline.win()
  if not valid_win(content) then
    return use_expected and expected_cmdline_box_rect() or nil
  end

  local router_ok, router = pcall(require, "noice.message.router")
  if router_ok and type(router.get_views) == "function" then
    for view in pairs(router.get_views()) do
      local nui = view._nui
      if nui ~= nil and nui.winid == content then
        local border = nui.border
        if border ~= nil and valid_win(border.winid) then
          live = win_rect(border.winid)
          return use_expected and merge_rects(live, expected_cmdline_box_rect()) or live
        end

        live = win_rect(content)
        return use_expected and merge_rects(live, expected_cmdline_box_rect()) or live
      end
    end
  end

  live = win_rect(content)
  return use_expected and merge_rects(live, expected_cmdline_box_rect()) or live
end

-- Screen columns (0-indexed) of the cmdline box's left and right borders. The
-- right value is the column the menu's right border should land on to merge;
-- the left value is the lowest column the menu may be pulled to. Returns
-- nil, nil when the box window can't be read.
local function cmdline_box_edges(use_expected)
  local box = cmdline_box_rect(use_expected)
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

local function cmdline_menu_is_open()
  local ok, menu = pcall(require, "blink.cmp.completion.windows.menu")
  return ok and menu.win ~= nil and menu.win:is_open() or false
end

local function update_cmdline_menu_position()
  if vim.api.nvim_get_mode().mode ~= "c" or not cmdline_menu_is_open() then
    return
  end

  local menu = require("blink.cmp.completion.windows.menu")
  pcall(menu.update_position)
end

local function redraw_cmdline()
  local ok, util = pcall(require, "noice.util")
  if ok and type(util.redraw) == "function" then
    pcall(util.redraw, { flush = true })
  elseif vim.api.nvim__redraw then
    pcall(vim.api.nvim__redraw, { flush = true })
  else
    pcall(vim.cmd.redraw)
  end
end

local cmdline_accept_reflow_pending = false

local function mark_cmdline_accept_reflow()
  cmdline_accept_reflow_pending = true
end

local function flush_cmdline_accept_reflow()
  if not cmdline_accept_reflow_pending or vim.api.nvim_get_mode().mode ~= "c" then
    return false
  end

  cmdline_accept_reflow_pending = false
  redraw_cmdline()
  return true
end

local cmdline_menu_reflow_pending = false

local function schedule_cmdline_menu_reflow()
  if cmdline_menu_reflow_pending then
    return
  end

  cmdline_menu_reflow_pending = true

  -- Reposition once, on the trailing edge, so the menu reflows in lockstep with
  -- noice's throttled box render instead of an eager leading pass that races
  -- ahead of it (which makes the merged right border jitter while typing).
  local delay = cmdline_reflow_delay()
  if delay > 0 then
    vim.defer_fn(function()
      cmdline_menu_reflow_pending = false
      update_cmdline_menu_position()
    end, delay)
  else
    vim.schedule(function()
      cmdline_menu_reflow_pending = false
      update_cmdline_menu_position()
    end)
  end
end

local function schedule_cmdline_accept_reflow()
  mark_cmdline_accept_reflow()
  vim.schedule(update_cmdline_menu_position)

  local delay = cmdline_reflow_delay()
  if delay > 0 then
    vim.defer_fn(update_cmdline_menu_position, delay)
  end
end

return {
  {
    "saghen/blink.cmp",
    init = function()
      local group = vim.api.nvim_create_augroup("DotfilesBlinkCmdlineMenu", { clear = true })
      vim.api.nvim_create_autocmd("User", {
        group = group,
        pattern = "BlinkCmpShow",
        callback = schedule_cmdline_menu_reflow,
      })
      -- Accepting a cmdline completion inserts the item text without re-firing
      -- BlinkCmpShow, so the menu keeps its pre-accept position while the noice
      -- cmdline box grows around the longer text and the merged right border
      -- drifts a column off. Reflow on CmdlineChanged as well so the menu
      -- re-merges with the box's new edge (this also covers the cmdline keymap
      -- preset, whose <Tab> never runs the custom accept-reflow keymap). Only
      -- when a menu is actually open, so plain commands and searches stay free.
      vim.api.nvim_create_autocmd("CmdlineChanged", {
        group = group,
        callback = function()
          if cmdline_menu_is_open() then
            schedule_cmdline_menu_reflow()
          end
        end,
      })
    end,
    opts = {
      -- Use LuaSnip as the snippet engine
      snippets = { preset = "luasnip" },

      -- Define Sources
      sources = {
        default = { "lsp", "path", "snippets", "buffer" },
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
            local use_expected_box = flush_cmdline_accept_reflow()

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
                  local box_left, box_right = cmdline_box_edges(use_expected_box)
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
            -- Command-line mode has no snippets, so <Tab> accepts the completion
            -- and keeps the noice cmdline reflow. In insert mode (below) <Tab> is
            -- a snippet-jump key; accepting is on <C-y>.
            if vim.api.nvim_get_mode().mode == "c" then
              if cmp.is_menu_visible() then
                mark_cmdline_accept_reflow()
                return cmp.select_and_accept({
                  callback = schedule_cmdline_accept_reflow,
                })
              end
              return
            end

            -- LuaSnip jumps 1 -> 2 -> ... -> 0, so on the highest-numbered stop
            -- the only target left is the exit ($0), which clangd places just
            -- inside the closing bracket. When that is all that remains, hop
            -- outside in one <Tab> instead of landing on it.
            local lok, ls = pcall(require, "luasnip")
            local closers =
              { [")"] = true, ["]"] = true, ["}"] = true, [">"] = true, ['"'] = true, ["'"] = true, ["`"] = true }
            local row, col = unpack(vim.api.nvim_win_get_cursor(0))
            local after = vim.api.nvim_get_current_line():sub(col + 1, col + 1)
            local in_insert = vim.api.nvim_get_mode().mode == "i"
            local jumpable = lok and ls.locally_jumpable(1)

            local only_exit_ahead = false
            if lok then
              pcall(function()
                local cur = ls.session.current_nodes[vim.api.nvim_get_current_buf()]
                local snip = cur and cur.parent and cur.parent.snippet
                if cur and snip and snip.insert_nodes then
                  if cur.pos == 0 then
                    only_exit_ahead = true
                  else
                    local maxpos = 0
                    for k in pairs(snip.insert_nodes) do
                      if type(k) == "number" and k > maxpos then
                        maxpos = k
                      end
                    end
                    only_exit_ahead = cur.pos == maxpos
                  end
                end
              end)
            end

            -- 1) Jump to the next real placeholder.
            if jumpable and not only_exit_ahead then
              return ls.jump(1)
            end

            -- 2) Nothing real to jump to: if the cursor sits before a closing
            --    delimiter, hop past it to leave the snippet. blink maps <Tab>
            --    as an <expr> (textlock), so dismiss the menu and move the
            --    cursor in a deferred callback.
            if in_insert and closers[after] then
              vim.schedule(function()
                cmp.hide()
                -- Advance the session onto the exit stop ($0) so LuaSnip's
                -- position matches where the cursor ends up. Without this it
                -- stays on the last placeholder and a later <S-Tab> jumps back
                -- from there, skipping it.
                pcall(function()
                  if ls.locally_jumpable(1) then
                    ls.jump(1)
                  end
                end)
                local r, c = unpack(vim.api.nvim_win_get_cursor(0))
                if closers[vim.api.nvim_get_current_line():sub(c + 1, c + 1)] then
                  pcall(vim.api.nvim_win_set_cursor, 0, { r, c + 1 })
                end
              end)
              return ""
            end

            -- 3) Only the exit stop is left and there is nothing to hop: jump.
            if jumpable then
              return ls.jump(1)
            end

            -- Nothing to jump: accept Copilot ghost text if it is showing. This
            -- is the only key that accepts a full suggestion; it never touches
            -- the completion menu.
            local ok, suggestion = pcall(require, "copilot.suggestion")
            if ok and suggestion.is_visible() then
              suggestion.accept()
              return true
            end
          end,
          "fallback",
        },
        ["<S-Tab>"] = {
          function()
            local lok, ls = pcall(require, "luasnip")
            if lok and ls.locally_jumpable(-1) then
              return ls.jump(-1)
            end

            -- Outdent in insert mode. blink's <expr> mapping does not feed a
            -- returned string, so send <C-d> through feedkeys.
            if vim.api.nvim_get_mode().mode == "i" then
              vim.api.nvim_feedkeys(
                vim.api.nvim_replace_termcodes("<C-d>", true, true, true),
                "n",
                false
              )
              return true
            end
          end,
          "fallback",
        },
      },
    },
  },
}
