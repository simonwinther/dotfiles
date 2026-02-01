vim.keymap.set("x", "<localleader>f", ":!column -t -s '|' -o ' | '<CR>", { desc = "Align file by pipe", buffer = true })
