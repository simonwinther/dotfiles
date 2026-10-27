local picker = {}

-- Each invocation opens a disposable floating terminal on the current workspace.
-- The launcher focuses an existing picker and closes the other mode, if open.
function picker.show()
    hl.exec_cmd("file-picker --window")
end

function picker.show_pdf()
    hl.exec_cmd("file-picker --window --pdf")
end

function picker.show_recent()
    hl.exec_cmd("file-picker --window --recent")
end

return picker
