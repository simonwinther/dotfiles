local headers = {
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "We have two lives, and the second begins               │
│       when we realize we only have one."                     │
│                                                              │
│                                           - Confucius        │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "The two most important days in your life               │
│       are the day you are born and the day                   │
│       you find out why."                                     │
│                                                              │
│                                           - Mark Twain       │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "To live is the rarest thing in the world.              │
│       Most people exist, that is all."                       │
│                                                              │
│                                           - Oscar Wilde      │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "Begin at once to live, and count each day              │
│       as a separate life."                                   │
│                                                              │
│                                           - Seneca           │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "Your time is limited, so don't waste it                │
│       living someone else's life."                           │
│                                                              │
│                                           - Steve Jobs       │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "Life isn't about finding yourself.                     │
│       Life is about creating yourself."                      │
│                                                              │
│                                     - George Bernard Shaw    │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "Live as if you were to die tomorrow.                   │
│       Learn as if you were to live forever."                 │
│                                                              │
│                                           - Gandhi           │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "And in the end, it's not the years in your             │
│       life that count. It's the life in your years."         │
│                                                              │
│                                           - Abraham Lincoln  │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "It is not death that a man should fear,                │
│       but he should fear never beginning to live."           │
│                                                              │
│                                           - Marcus Aurelius  │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "Dare to live the life you have dreamed                 │
│       for yourself. Go forward and make it true."            │
│                                                              │
│                                           - R.W. Emerson     │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "Never let the fear of striking out                     │
│       keep you from playing the game."                       │
│                                                              │
│                                           - Babe Ruth        │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "Life is what happens to you while you're               │
│       busy making other plans."                              │
│                                                              │
│                                           - John Lennon      │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "The purpose of our lives is to be happy."              │
│                                                              │
│                                           - Dalai Lama       │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "The unexamined life is not worth living."              │
│                                                              │
│                                           - Socrates         │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│      "Life is pure adventure, and the sooner we              │
│       realize that, the quicker we will treat it as art."    │
│                                                              │
│                                           - Maya Angelou     │
│                                                              │
└──────────────────────────────────────────────────────────────┘
  ]],
  [[
┌──────────────────────────────────────────────────────────────┐
│          ███████╗██╗███╗   ███╗ ██████╗ ███╗   ██╗           │
│          ██╔════╝██║████╗ ████║██╔═══██╗████╗  ██║           │
│          ███████╗██║██╔████╔██║██║   ██║██╔██╗ ██║           │
│          ╚════██║██║██║╚██╔╝██║██║   ██║██║╚██╗██║           │
│          ███████║██║██║ ╚═╝ ██║╚██████╔╝██║ ╚████║           │
│          ╚══════╝╚═╝╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝           │
│                 > SYSTEM READY // HELLO SIMON                │
└──────────────────────────────────────────────────────────────┘
  ]],
}

math.randomseed(os.time())
local selected_header = headers[math.random(#headers)]

return {
  {
    "folke/snacks.nvim",
    opts = {
      dashboard = {
        preset = {
          header = selected_header,
        },
      },
      image = {
        doc = {
          max_width = 40,
          max_height = 20,
          enabled = true,
        },
        enabled = true,
        math = {
          enabled = false,
        },
      },
      explorer = { enabled = false }, -- disable built-in explorer (i use neotree)
      picker = {
        actions = {
          inspect_picker = function(picker)
            vim.print(picker)
          end,
          pivot_smart = function(picker)
            local is_grep = picker.opts.source == "grep"

            local current_text = (picker.input.filter.search ~= "" and picker.input.filter.search)
              or picker.input.filter.pattern

            local show_hidden = picker.opts.hidden
            local current_cwd = picker.input.filter.cwd

            picker:close()

            if is_grep then
              Snacks.picker.files({
                pattern = current_text,
                hidden = show_hidden,
                cwd = current_cwd,
              })
            else
              Snacks.picker.grep({
                search = current_text,
                hidden = show_hidden,
                cwd = current_cwd,
              })
            end
          end,
        },
        win = {
          input = {
            keys = {
              ["<M-e>"] = { "pivot_smart", mode = { "i", "n" }, desc = "Switch Grep/Files" },
            },
          },
        },
        layouts = {
          -- https://github.com/folke/snacks.nvim/blob/main/docs/picker.md#%EF%B8%8F-layouts
          -- copied from docs, and slight changes :)
          my_telescope = {
            layout = {
              reverse = true,
              box = "horizontal",
              backdrop = false,
              width = 0.9,
              height = 0.9,
              border = "none",
              {
                box = "vertical",
                { win = "list", title = " Results ", title_pos = "center", border = true },
                { win = "input", height = 1, border = true, title = "{title} {live} {flags}", title_pos = "center" },
              },
              {
                win = "preview",
                title = "{preview:Preview}",
                width = 0.60,
                border = true,
                title_pos = "center",
              },
            },
          },
          tall_ivy = {
            layout = {
              box = "vertical",
              backdrop = false,
              row = -1,
              width = 0,
              height = 0.85,
              border = "top",
              title = " {title} {live} {flags}",
              title_pos = "left",
              { win = "input", height = 1, border = "bottom" },
              {
                box = "horizontal",
                { win = "list", border = "none" },
                { win = "preview", title = "{preview}", width = 0.65, border = "left" },
              },
            },
          },
        },
        layout = {
          preset = "my_telescope", -- Or "center", "top", "bottom", "single"
        },
        enabled = true,
        formatters = {
          file = {
            filename_first = true,
          },
        },
      },
    },
  },
}
