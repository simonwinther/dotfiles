---
name: canvas-active-courses-due
description: View active Canvas courses and upcoming due assignments in one response, including local due times and submission status. Use when a user asks what classes are currently active, what is due soon, or requests a quick deadline check across Canvas courses.
---

# Canvas Active Courses Due

1. List active courses with `mcp__canvas__list_courses`.
2. Retrieve upcoming assignments with `mcp__canvas__get_my_upcoming_assignments`.
3. Retrieve submission status with `mcp__canvas__get_my_submission_status`.
4. Match assignments to courses and mark each item as submitted, missing, or pending.
5. Convert due dates to the user timezone when available and present exact calendar dates.
6. Sort by due date ascending and group by course for readability.

## Output Format

Return:
- Active course list (name, term or state if available)
- Upcoming assignment list grouped by course
- For each assignment: due date/time, submission status, points if available, and direct urgency label (`overdue`, `due soon`, `upcoming`)
- A short priority summary of the next 3 deadlines

## Rules

- If no assignments are due in the selected window, explicitly report that no upcoming deadlines were found.
- If data is missing (due date, points, status), print `unknown` instead of omitting the field.
- Prefer concise tables or bullet lists over long prose.
