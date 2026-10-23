(() => {
  const PROJECT_ID = /^g-p-[0-9a-f]{32}$/;
  const LIST_PATH = "/backend-api/gizmos/snorlax/sidebar";

  const failure = (code, message) => Object.assign(new Error(message), { code });

  function errorCode(error) {
    if (typeof error?.code === "string") return error.code;
    if (error?.name === "TimeoutError") return "timeout";
    if (error?.name === "TypeError") return "network";
    if (error?.name === "SyntaxError") return "invalid_response";
    return "unexpected";
  }

  function page(data) {
    if (!data || (!Array.isArray(data.items) && !Array.isArray(data.gizmos)) ||
        !(data.cursor === null || (typeof data.cursor === "string" && data.cursor.length > 0))) {
      throw failure("invalid_response", "The website returned an unsupported project list.");
    }
    const raw = [
      ...(data.items || []).map((item) => item?.gizmo?.gizmo || item?.gizmo),
      ...(data.gizmos || []).map((item) => item?.gizmo || item),
    ];
    const projects = [];
    for (const project of raw) {
      if (!project || typeof project.id !== "string") throw failure("invalid_response", "A project record is incomplete.");
      if (!PROJECT_ID.test(project.id)) {
        if (project.gizmo_type === "snorlax" || !project.id.startsWith("g-")) {
          throw failure("invalid_response", "The website returned an unsupported project ID.");
        }
        continue;
      }
      const name = project.display?.name || project.name;
      if (typeof name !== "string" || !name.trim()) throw failure("invalid_response", "A project is missing its name.");
      projects.push({ id: project.id, name, url: `https://chatgpt.com/g/${project.id}/project` });
    }
    return { projects, cursor: data.cursor };
  }

  function accountCookie(cookie) {
    const value = cookie.match(/(?:^|;\s*)_account=([^;]*)/)?.[1];
    return value ? decodeURIComponent(value) : "";
  }

  async function collect({ request, cookie, now = Date.now, signal }) {
    const startedAt = now();
    const selectedAccount = accountCookie(cookie());
    const session = await request("/api/auth/session");
    const accountId = session.user?.id;
    const token = session.accessToken || session.access_token;
    if (typeof accountId !== "string" || !accountId || typeof token !== "string" || !token) {
      throw failure("auth_required", "Sign in to the website to sync projects.");
    }
    const headers = { Authorization: `Bearer ${token}` };
    let workspaceId = "personal";
    let workspaceName = "Personal";
    if (selectedAccount) {
      const accounts = await request("/backend-api/accounts/check/v4-2023-04-27", headers);
      const account = accounts.accounts?.[selectedAccount]?.account ||
        Object.values(accounts.accounts || {}).map((entry) => entry.account)
          .find((entry) => entry?.account_id === selectedAccount);
      if (typeof account?.account_id !== "string" || !account.account_id) {
        throw failure("workspace_unknown", "Could not identify the selected workspace.");
      }
      workspaceId = account.account_id;
      workspaceName = account.name || "Personal";
      headers["ChatGPT-Account-ID"] = workspaceId;
    }
    const projects = new Map();
    const cursors = new Set();
    let cursor = null;
    do {
      signal?.throwIfAborted();
      if (cursors.size >= 1000) throw failure("pagination", "Project pagination did not finish.");
      const query = new URLSearchParams({ owned_only: "false", conversations_per_gizmo: "0" });
      if (cursor) query.set("cursor", cursor);
      const current = page(await request(`${LIST_PATH}?${query}`, headers));
      for (const project of current.projects) projects.set(project.id, project);
      cursor = current.cursor;
      if (cursor && cursors.has(cursor)) throw failure("pagination", "Project pagination repeated a cursor.");
      if (cursor) cursors.add(cursor);
    } while (cursor);
    const finalSession = await request("/api/auth/session");
    if (finalSession.user?.id !== accountId || accountCookie(cookie()) !== selectedAccount) {
      throw failure("account_changed", "The account or workspace changed during sync. Try again.");
    }
    signal?.throwIfAborted();
    return {
      version: 1, complete: true, started_at: startedAt,
      account_id: accountId, workspace_id: workspaceId, workspace_name: workspaceName,
      projects: [...projects.values()],
    };
  }

  function requester(fetcher, signal) {
    return async (path, headers = {}) => {
      const response = await fetcher(path, {
        method: "GET", credentials: "include", cache: "no-store", redirect: "error",
        headers: { Accept: "application/json", ...headers },
        signal: AbortSignal.any([signal, AbortSignal.timeout(30000)]),
      });
      if (!response.ok) {
        await response.body?.cancel();
        throw failure(`http_${response.status}`, `Project sync request failed (HTTP ${response.status}).`);
      }
      return response.json();
    };
  }

  globalThis.ProjectPickerAPI = { page, collect, requester, errorCode };
})();
