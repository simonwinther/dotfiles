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
        enabled = true,
      },
      explorer = { enabled = false }, -- disable built-in explorer (i use neotree)
      picker = {
        enabled = true,
      },
    },
  },
}
