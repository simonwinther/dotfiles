local core = require("utils.core")

local function get_terminal_window(name)
  local buf = vim.fn.bufnr(name)

  -- Wipe buffer if it exists but is 'dead'
  if buf ~= -1 and not vim.api.nvim_buf_is_valid(buf) then
    vim.cmd("bwipeout! " .. buf)
    buf = -1
  end

  if buf == -1 then
    buf = vim.api.nvim_create_buf(false, true)
    vim.api.nvim_buf_set_name(buf, name)
  end

  -- Open split if closed, otherwise focus it
  if vim.fn.bufwinid(buf) == -1 then
    vim.cmd("botright vsplit")
    vim.api.nvim_win_set_buf(0, buf)
  else
    vim.api.nvim_set_current_win(vim.fn.bufwinid(buf))
  end
end

local function run_python(is_visual)
  local path = vim.api.nvim_buf_get_name(0)
  local is_temp = false

  if is_visual then
    -- Make sure core.get_visual_selection is available
    local lines = core.get_visual_selection()
    path = vim.fn.tempname() .. ".py"
    vim.fn.writefile(lines, path)
    is_temp = true
  else
    vim.cmd("write")
    if path == "" then
      return print("Error: Save file first")
    end
  end

  get_terminal_window("PYTHON_RUNNER")

  vim.fn.jobstart({ "python3", "-u", path }, {
    term = true,
    on_exit = function(_, code)
      if code ~= 0 then
        print("Exit Code: " .. code)
      end
      if is_temp then
        vim.fn.delete(path)
      end
    end,
  })

  vim.cmd("startinsert")
end

-- Buffer-local mappings
vim.keymap.set("n", "<localleader>r", function()
  run_python(false)
end, { buffer = true, desc = "Run File" })
vim.keymap.set("v", "<localleader>r", function()
  run_python(true)
end, { buffer = true, desc = "Run Selection" })

-- PyDocs
vim.keymap.set("n", "<localleader>d", function()
  vim.ui.input({ prompt = "pydoc: " }, function(query)
    if not query or query == "" then
      return
    end

    local buf = vim.api.nvim_create_buf(false, true)

    local width = math.floor(vim.o.columns * 0.85)
    local height = math.floor(vim.o.lines * 0.80)
    local row = math.floor(vim.o.lines * 0.10)
    local col = math.floor(vim.o.columns * 0.075)

    vim.api.nvim_open_win(buf, true, {
      relative = "editor",
      width = width,
      height = height,
      row = row,
      col = col,
      style = "minimal",
      border = "rounded",
    })

    vim.cmd("silent read !python -m pydoc " .. vim.fn.shellescape(query))
    vim.cmd("1delete _")
    vim.bo[buf].modifiable = false

    vim.keymap.set("n", "q", "<cmd>close<cr>", { buffer = buf, silent = true })
  end)
end, {
  buffer = true,
  desc = "Search pydoc",
})
