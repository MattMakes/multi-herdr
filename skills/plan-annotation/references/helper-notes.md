# Helper notes

How the bundled review helper behaves. The code is in
`script-review-helper.md`. The skill text says when to start it.

## Command line

    review_helper.py [--gate] [--json] [--] TARGET

- `TARGET`: exactly one argument, a file or a folder on this machine.
- `--gate`: adds the approve control to the page. Without this flag the page
  has no such control and the helper refuses an approve message.
- `--json`: prints the result as one JSON object. It adds no control.
- `--`: everything after it is taken as the target, even when it begins with
  two dashes.

No other option exists. A second target, or none, is an argument error.

## Serving rules

- The server binds `127.0.0.1` and lets the system pick a free port for each
  session. No other interface is bound.
- Each session draws a fresh random key. A request is answered only when its
  query string carries that key exactly once and its `Host` header names the
  loopback address or `localhost` with the session's port. Every other
  request gets status 403 and no document text, and the open session goes on
  untouched.
- The helper makes no outbound request and asks no name service.
- Routes: the page itself, one document by its list number, the final
  message of the page, and the "page was left" signal. No route takes a file
  name or a path, so only listed documents can be fetched.
- Listening stops as soon as the session ends. The helper keeps no file, no
  log and no cache.
- The target is opened for reading only.

## How documents are shown

- The page inserts document text as plain text nodes. Markup inside a
  document is never interpreted, and scripts inside it never run.
- A content security policy allows only the page's own script and style,
  each tied to a random token of the session. Inline handlers, remote
  resources, frames and form posts are blocked.
- Markdown gets light styling of heading lines and fenced code lines. The
  characters stay as written.
- An HTML file is reduced to its visible text before it is sent: tags become
  line breaks, and the content of script, style, template and noscript
  elements is dropped.
- Every other text file is shown line by line as it is.

## Supported files

A file is reviewable when all of these hold:

- it is a regular file;
- it has at most 2 MB (2 097 152 bytes);
- it holds no NUL byte;
- it decodes as UTF-8 (a byte order mark is accepted).

Markdown, JSON, YAML, INI, TOML, CSV, logs, source code and HTML pass. Images,
archives, office files and other binary formats are refused, and so is text
in a legacy encoding. Convert such text to UTF-8 first.

## Folder review

- The folder is walked with its subfolders. Documents are listed in name
  order, each under its location relative to the folder.
- Left out: entries whose name starts with a dot, folders reached through a
  link, files reached through a link that ends outside the folder, and files
  that fail the rules above.
- A listed file is checked again on every fetch. If it has turned into a link
  that points outside, or has stopped being reviewable, the fetch fails.
- At most 300 documents are listed. The page says so when the folder holds
  more.
- A folder with nothing reviewable is refused with exit status 4.

## Page controls

- Mark text in the document, press "Comment on selection", write the comment,
  press "Add".
- "Comment on whole document" adds a general comment on the open document.
- Each listed annotation has a "Remove" button.
- "Submit feedback" sends the list and ends the session. In a gate session
  this button reads "Request changes". It is disabled while the list is
  empty.
- "Close without feedback" ends the session with nothing sent. The page asks
  for confirmation first when annotations are listed.
- Gate sessions only: "Approve..." opens a box for an optional note, and
  "Confirm approval" ends the session as approved.

## How a session ends

| Event | Result |
|---|---|
| the reviewer submits one or more annotations | annotated |
| the reviewer confirms approval (gate session) | approved |
| the reviewer presses close | dismissed |
| the tab is closed or navigates away and does not come back within 3 seconds | dismissed |
| the helper receives an interrupt, a terminate or a hang-up signal | dismissed |

A reload inside the 3 seconds keeps the session open. Annotations that were
not yet sent are lost on a reload, and the browser warns before that happens.

There is no time limit. The helper waits as long as the page stays open.

## When no browser starts

The helper writes one line to standard error that contains the page address,
and keeps waiting. Whoever reviews can open that address in a browser on the
same machine. The address holds the session key, so give it only to the
reviewer.

## Limits

| Limit | Value |
|---|---|
| size of one document | 2 MB |
| documents listed for a folder | 300 |
| annotations in one result | 2000 |
| length of one comment or note | 20 000 characters |
| length of a quoted passage | 4000 characters, longer ones are cut |
