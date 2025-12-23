-- Autocmds are automatically loaded on the VeryLazy event
-- Default autocmds that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/autocmds.lua
--
-- Add any additional autocmds here
-- with `vim.api.nvim_create_autocmd`
--
-- Or remove existing autocmds by their group name (which is prefixed with `lazyvim_` for the defaults)
-- e.g. vim.api.nvim_del_augroup_by_name("lazyvim_wrap_spell")

-- Track the last synced line globally in this file
-- Track the last line we synced to prevent spam
vim.g.vimtex_autoview_scroll = true

vim.api.nvim_create_user_command("VimtexAutoScroll", function()
  vim.g.vimtex_autoview_scroll = not vim.g.vimtex_autoview_scroll
  vim.notify("VimTeX auto-scroll " .. (vim.g.vimtex_autoview_scroll and "enabled" or "disabled"))
end, {})

local grp = vim.api.nvim_create_augroup("VimtexViewOnScroll__user", { clear = true })

local last_topline = {} -- per-window: last topline we synced at
local timers = {} -- per-window: debounce timer

vim.api.nvim_create_autocmd("WinScrolled", {
  group = grp,
  callback = function(args)
    if not vim.g.vimtex_autoview_scroll then
      return
    end
    if vim.bo[args.buf].filetype ~= "tex" then
      return
    end
    if vim.b[args.buf].vimtex == nil then
      return
    end

    local win = args.win or vim.api.nvim_get_current_win()
    if not vim.api.nvim_win_is_valid(win) then
      return
    end

    -- cheapest "top of window" line
    local topline = vim.api.nvim_win_call(win, function()
      return vim.fn.line("w0")
    end)

    -- dedupe: ignore tiny movements
    local last = last_topline[win]
    if last and math.abs(topline - last) < 10 then
      return
    end
    last_topline[win] = topline

    -- debounce: avoid spamming VimtexView during fast scroll
    if timers[win] then
      timers[win]:stop()
      timers[win]:close()
    end

    local t = vim.loop.new_timer()
    timers[win] = t
    t:start(
      60,
      0,
      vim.schedule_wrap(function()
        -- re-check minimal stuff; buffer/window might have changed
        if not vim.g.vimtex_autoview_scroll then
          return
        end
        if not vim.api.nvim_win_is_valid(win) then
          return
        end
        if not vim.api.nvim_buf_is_valid(args.buf) then
          return
        end
        if vim.bo[args.buf].filetype ~= "tex" then
          return
        end
        if vim.b[args.buf].vimtex == nil then
          return
        end
        vim.cmd("silent! VimtexView")
        vim.defer_fn(function()
          vim.fn.jobstart({
            "hyprctl",
            "dispatch",
            "focuswindow",
            "class:^(com\\.mitchellh\\.ghostty)$",
          }, { detach = true })
        end, 30)
      end)
    )
  end,
})

-- (optional but nice) cleanup when a window closes
vim.api.nvim_create_autocmd("WinClosed", {
  group = grp,
  callback = function(args)
    local win = tonumber(args.match)
    last_topline[win] = nil
    if timers[win] then
      timers[win]:stop()
      timers[win]:close()
      timers[win] = nil
    end
  end,
})
