/**
 * farseer delegation for pi and omp.
 *
 * `31 manager delegation reach` found farseer's roster reachable from Claude
 * Code and from nowhere else, because delegation had only ever been offered as
 * an MCP endpoint and pi has no MCP client. A manager here was handed a goal
 * and no way to hand any of it on - and one asked to delegate ran the work
 * itself and reported it as a delegation, because that is the closest thing to
 * compliance an agent with no delegate tool can reach.
 *
 * This registers the two verbs against farseer's plain-JSON manager face at
 * `/v1/manager/delegate/*`, which calls the same functions the MCP tools call.
 *
 * Nothing is registered unless farseer put a live manager identity in the
 * environment, so a pi session the operator started themselves never sees a
 * tool it could not use.
 *
 * The credentials are read from the environment and never appear in a tool
 * schema: the model states a worker and a goal, and cannot read, spend or leak
 * the token that authorizes the call. That is strictly better than the MCP
 * shape, where the manager carries its own bearer in its prompt.
 *
 * No `import` and no `pi.zod`. Probed on 2026-08-28: pi 0.84.3's ExtensionAPI
 * has 26 methods and a schema builder is not among them - `parameters` is
 * plain JSON Schema - and an extension that reached for one failed to load,
 * which killed the runner before its first turn. `10 runner inventory`'s rule
 * applies to a harness's extension API as much as to its output.
 */
const endpoint = process.env.FARSEER_ENDPOINT;
const runId = process.env.FARSEER_MANAGER_RUN_ID;
const token = process.env.FARSEER_MANAGER_TOKEN;

async function post(path: string, body: Record<string, unknown>): Promise<string> {
	const response = await fetch(`${endpoint}${path}`, {
		method: "POST",
		headers: { "content-type": "application/json", authorization: `Bearer ${token}` },
		body: JSON.stringify({ manager_run_id: runId, manager_token: token, ...body }),
	});
	const text = await response.text();
	if (!response.ok) {
		// Returned as the tool's own result rather than thrown: a refusal is
		// farseer's answer - the roster does not name that worker, the cell is
		// at its worker cap, the budget is spent - and a manager should read it
		// and adapt, not see a transport failure it cannot act on.
		return `farseer refused this delegation (HTTP ${response.status}): ${text}`;
	}
	return text;
}

export default function farseerDelegate(pi: any) {
	if (!endpoint || !runId || !token) {
		return;
	}

	pi.registerTool({
		name: "delegate_to_worker",
		label: "Delegate",
		description:
			"Delegate a precise sub-goal to a named worker in this cell's roster and wait for it to finish. " +
			"The worker runs in its own workspace with its own runner, and its budget is drawn from yours. " +
			"Returns the worker's terminal text, its outcome, and what it spent. " +
			"Only the workers named in your roster are callable.",
		parameters: {
			type: "object",
			required: ["worker", "goal"],
			properties: {
				worker: { type: "string", description: "A worker named in this cell's roster." },
				goal: {
					type: "string",
					description:
						"The precise sub-goal for this worker. It sees nothing of this conversation, so it must stand alone.",
				},
				definition_of_done: {
					type: "string",
					description: "What the worker must produce for the sub-goal to count as finished.",
				},
			},
		},
		async execute(_toolCallId: string, params: { worker: string; goal: string }) {
			const text = await post("/v1/manager/delegate/worker", params);
			return { content: [{ type: "text", text }], details: { worker: params.worker } };
		},
	});

	pi.registerTool({
		name: "delegate_to_cell",
		label: "Call cell",
		description:
			"Call another cell named in this cell's roster. Fire-and-forget: this returns a call_id and the " +
			"callee's run_id immediately, not an answer. The callee owns its own workspace, runner and tools; " +
			"you state the goal and the ceiling. Only the cells named in your roster are callable.",
		parameters: {
			type: "object",
			required: ["cell", "goal"],
			properties: {
				cell: { type: "string", description: "A cell named in this cell's roster." },
				goal: { type: "string", description: "The goal for the callee's own manager." },
				definition_of_done: { type: "string", description: "What the callee must produce." },
				autonomy_ceiling: {
					type: "string",
					enum: ["reversible", "undoable", "irreversible"],
					description: "A ceiling only ever narrows: naming one above the roster entry lowers to theirs.",
				},
			},
		},
		async execute(_toolCallId: string, params: { cell: string; goal: string }) {
			const text = await post("/v1/manager/delegate/cell", params);
			return { content: [{ type: "text", text }], details: { cell: params.cell } };
		},
	});
}
