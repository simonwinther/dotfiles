---
name: canvas-assignment-to-latex
description: Extract a Canvas assignment from a specific course and generate a complete LaTeX document from assignment details, instructions, dates, points, submission settings, and optional rubric criteria. Also split clearly separated tasks and subtasks into structured text files and preserve verbatim problem statements from assignment PDFs without changing wording. Use when asked to export or convert Canvas assignments to LaTeX, build printable assignment sheets, produce syllabus-quality assignment handouts from Canvas LMS data, or decompose assignment task sets into per-task files.
---

# Canvas Assignment to LaTeX

Export one Canvas assignment into a standalone `.tex` document that is ready to compile.

## Prerequisites

- Ensure Canvas MCP is connected and authenticated.
- Ensure the user has access to the target course and assignment.
- Ensure output should be in LaTeX, not Markdown or HTML.

## Workflow

### 1. Resolve course and assignment

1. If the user provides only a course name/code, call `list_courses` to resolve the course.
2. Call `list_assignments(course_identifier)` to find the target assignment.
3. Match assignment by exact title first; if multiple close matches exist, ask for confirmation.
4. Store:
   - `course_identifier`
   - `assignment_id`
   - `assignment_name`

### 2. Extract assignment source data

1. Call `get_assignment_details(course_identifier, assignment_id)`.
2. Optionally call `list_assignment_rubrics(course_identifier, assignment_id)`.
3. If rubric data exists and detailed criteria are needed, call `get_assignment_rubric_details(course_identifier, assignment_id)`.
4. If the assignment includes attached PDFs, capture attachment metadata and file IDs.

Collect at minimum:
- Assignment title
- Course name/code
- Description HTML
- Due, unlock, and lock datetimes
- Points possible
- Grading type
- Submission types
- Allowed file extensions
- Peer review settings
- Rubric criteria and points (if present)
- Attachment file IDs and names (if present)

### 3. Convert rich text safely

1. Transform Canvas HTML description into LaTeX-safe text.
2. Preserve headings, paragraphs, ordered/unordered lists, links, emphasis, and code blocks.
3. Escape LaTeX-sensitive characters: `\`, `{`, `}`, `%`, `$`, `#`, `&`, `_`, `~`, `^`.
4. Convert links to `\href{url}{label}`.
5. Convert line breaks and spacing into readable LaTeX paragraphs.
6. If content cannot be converted cleanly, include a short note in comments and preserve plain text.

### 4. Extract verbatim problem statement from PDF

Apply this section when assignment tasks are in an attached PDF.

1. Download each relevant assignment PDF with `download_course_file(course_identifier, file_id)`.
2. Extract text in source order.
3. Preserve the problem statement exactly as written in the source PDF:
   - Do not paraphrase.
   - Do not rewrite notation.
   - Do not change wording.
   - Do not silently fix grammar.
4. Keep punctuation, symbols, numbering labels, and mathematical tokens identical.
5. If extraction quality is uncertain (OCR noise, missing glyphs, broken formulas), stop and flag the issue instead of inventing text.
6. Treat verbatim fidelity as mandatory for problem statements.

### 5. Split tasks and subtasks into files

Apply this section when the assignment includes multiple clearly separated tasks.

1. Create one output directory per assignment:
   - `output/<assignment-slug>/`
2. Write full verbatim source problem statement to:
   - `output/<assignment-slug>/00-problem-statement/problem-statement.txt`
3. For each top-level task, create:
   - `output/<assignment-slug>/task-<nn>-<task-slug>/task.txt`
4. For each clearly separated subtask under a task, create:
   - `output/<assignment-slug>/task-<nn>-<task-slug>/subtask-<label>.txt`
5. Preserve the original text one-to-one in `task.txt` and each `subtask-*.txt`.
6. Keep related content together when it is not clearly separable.
7. Use lowercase kebab-case slugs, zero-padded numeric indices, and stable ordering from source.
8. Also generate an index file:
   - `output/<assignment-slug>/INDEX.txt`
   - Include path, task number, subtask label, and first line preview.

## Output format

Write a single `.tex` file using this structure:

```tex
\documentclass[11pt]{article}
\usepackage[margin=1in]{geometry}
\usepackage[T1]{fontenc}
\usepackage[utf8]{inputenc}
\usepackage{lmodern}
\usepackage{hyperref}
\usepackage{enumitem}
\usepackage{longtable}
\usepackage{xcolor}

\hypersetup{
  colorlinks=true,
  linkcolor=blue,
  urlcolor=blue
}

\title{<Assignment Title>}
\author{<Course Code or Course Name>}
\date{<Generated date or Due date>}

\begin{document}
\maketitle

\section*{Assignment Summary}
\begin{itemize}[leftmargin=1.5em]
  \item \textbf{Points:} <points possible or N/A>
  \item \textbf{Grading Type:} <grading type>
  \item \textbf{Due:} <localized due datetime or Not set>
  \item \textbf{Available From:} <unlock datetime or Not set>
  \item \textbf{Available Until:} <lock datetime or Not set>
  \item \textbf{Submission Types:} <comma-separated submission types>
  \item \textbf{Allowed Extensions:} <comma-separated extensions or Any>
  \item \textbf{Peer Reviews:} <enabled/disabled + automatic/manual if available>
\end{itemize}

\section*{Instructions}
<converted description body and/or verbatim task text>

<optional rubric section>

\end{document}
```

## Optional rubric section

If rubric data exists, append:

```tex
\section*{Rubric}
\begin{longtable}{p{0.45\textwidth} p{0.15\textwidth} p{0.32\textwidth}}
\textbf{Criterion} & \textbf{Points} & \textbf{Performance Levels} \\
\hline
<one row per criterion>
\end{longtable}
```

Populate `Performance Levels` with compact rating labels and point values.

## Date and timezone handling

1. Convert Canvas ISO datetimes to the user’s local timezone when known.
2. Use explicit dates in output, for example: `March 6, 2026 at 11:59 PM`.
3. If a date is missing, print `Not set`.

## File naming convention

- Assignment folder: `<assignment-slug>` from assignment title.
- Task folder: `task-01-<slug>`, `task-02-<slug>`, ...
- Subtask file labels:
  - Alphabetic labels: `subtask-a.txt`, `subtask-b.txt`
  - Numeric labels: `subtask-1.txt`, `subtask-2.txt`
  - Fallback: `subtask-01.txt`, `subtask-02.txt`

## Quality checks before finalizing

1. Ensure every LaTeX special character is escaped.
2. Ensure environments are balanced (`\begin`/`\end`).
3. Ensure assignment description is not empty; if empty, state `No description provided in Canvas.`
4. Ensure generated file compiles in principle (no obvious syntax errors).
5. Ensure no Canvas HTML tags remain in final content.
6. Ensure verbatim task and subtask files are one-to-one with source text.
7. Ensure task split preserves original ordering and labels.
8. Ensure no task text is merged when clearly separate in source.

## Example user prompts this skill should handle

- "Convert Assignment 3 from BIO101 into LaTeX."
- "Extract the Midterm Project assignment from course 12345 and generate a .tex handout."
- "Make a printable LaTeX version of my Canvas assignment with rubric included."
- "Split this assignment into task and subtask text files while keeping the original wording exactly."
