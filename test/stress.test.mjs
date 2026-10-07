import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const exec = promisify(execFile);
const root = fileURLToPath(new URL("../", import.meta.url));
const temporary = join(root, ".temp/stress-tests");

async function fixture() {
  await mkdir(temporary, { recursive: true });
  return mkdtemp(join(temporary, "run-"));
}

async function report(directory, mode = "peak", restarts = 0) {
  const { stdout } = await exec("python3", [
    join(root, "test/stress/report.py"),
    "--directory",
    directory,
    "--mode",
    mode,
    "--profile",
    "2c4g",
    "--stacks",
    "http",
    "kv",
    "--run-id",
    "fixture",
    "--timestamp",
    "2026-10-08T00:00:00Z",
    "--restarts",
    String(restarts),
  ]);
  return JSON.parse(stdout);
}

test("P0, peak and soak reject failed requests, excessive latency, and empty stacks", async () => {
  const directory = await fixture();
  for (const stack of ["http", "kv"]) {
    await writeFile(join(directory, `lat-${stack}.txt`), "10\n20\n30\n");
    await writeFile(join(directory, `err-${stack}.txt`), "");
  }
  await writeFile(join(directory, "soak-events.jsonl"), "");
  assert.equal((await report(directory)).verdict, "pass");
  assert.equal((await report(directory, "soak")).verdict, "pass");
  await writeFile(
    join(directory, "err-http.txt"),
    "GET / 500\nGET / 500\nGET / 500\n",
  );
  for (const mode of ["p0", "peak", "soak"]) {
    const failed = await report(directory, mode);
    assert.equal(failed.verdict, "fail");
    assert.equal(failed.stacks.http.error_rate, 1);
    assert.equal(failed.stacks.http.verdict, "fail");
  }
  await writeFile(join(directory, "err-http.txt"), "");
  await writeFile(join(directory, "lat-http.txt"), "10000\n");
  assert.equal((await report(directory, "soak")).verdict, "fail");
  await writeFile(join(directory, "lat-http.txt"), "");
  const empty = await report(directory, "soak");
  assert.equal(empty.verdict, "fail");
  assert.ok(
    empty.stacks.http.anomalies.some((item) => item.type === "no_samples"),
  );
  await assert.rejects(report(directory, "soak", 1), /restart count/);
  await rm(directory, { recursive: true });
});

test("stress restart selects the Compose service and propagates failed or ineffective restarts", async () => {
  const directory = await fixture();
  const trace = join(directory, "trace");
  const started = join(directory, "started");
  await writeFile(
    join(directory, "docker"),
    `#!/bin/sh
printf '%s\n' "$*" >> "$TRACE"
case "$*" in
  *" ps -q ocd") printf '%s\n' fixture-container ;;
  *" restart ocd")
    [ "\${FAIL_RESTART:-0}" != 1 ] || exit 12
    [ "\${NO_RESTART:-0}" = 1 ] || touch "$STARTED"
    ;;
  "inspect --format {{.State.StartedAt}} fixture-container")
    if [ -f "$STARTED" ]; then echo after; else echo before; fi ;;
  *) exit 13 ;;
esac
`,
    { mode: 0o755 },
  );
  for (const command of ["pgrep", "pkill"]) {
    await writeFile(
      join(directory, command),
      `#!/bin/sh\necho unexpected-host-process-access >> "$TRACE"\nexit 99\n`,
      { mode: 0o755 },
    );
  }
  const env = {
    ...process.env,
    PATH: `${directory}:${process.env.PATH}`,
    TRACE: trace,
    STARTED: started,
    OPEN_COMPUTE_ROOT: root,
    STRESS_ACCOUNT_ID: "fixture-account",
  };
  const script = `. '${join(root, "test/stress/common.sh")}'; restart_stress_container`;
  await exec("bash", ["-c", script], { env });
  const calls = await readFile(trace, "utf8");
  assert.match(calls, /compose .* ps -q ocd/);
  assert.match(calls, /inspect .*fixture-container/);
  assert.match(calls, /compose .* restart ocd/);
  assert.doesNotMatch(calls, /open-compute-ocd|unexpected-host-process-access/);
  await assert.rejects(
    exec("bash", ["-c", script], { env: { ...env, FAIL_RESTART: "1" } }),
    { code: 1 },
  );
  await rm(started);
  await assert.rejects(
    exec("bash", ["-c", script], { env: { ...env, NO_RESTART: "1" } }),
    /did not restart/,
  );
  await rm(directory, { recursive: true });
});

test("final qualification exits unsuccessfully and preserves failed evidence", async () => {
  const directory = await fixture();
  const result = join(directory, "result.json");
  const script = `. '${join(root, "test/stress/lib/anomaly-check.sh")}'; finalize_verdict '${result}'`;
  const env = {
    ...process.env,
    STRESS_RUN_DIR: directory,
    STRESS_BASE_URL: "http://127.0.0.1:8788",
  };
  await writeFile(
    result,
    JSON.stringify({ verdict: "fail", stacks: { http: { errors: 1 } } }),
  );
  await assert.rejects(exec("bash", ["-c", script], { env }), { code: 1 });
  assert.equal(
    JSON.parse(await readFile(join(directory, "failed/result.json"), "utf8"))
      .verdict,
    "fail",
  );
  assert.equal(
    JSON.parse(await readFile(join(directory, "anomalies.json"), "utf8"))
      .verdict,
    "fail",
  );
  await rm(directory, { recursive: true });
});

test("queue peak verifies a sent label without reading the sender's local counter", async () => {
  const directory = await fixture();
  const source = await readFile(join(root, "test/stress/stress-p1.sh"), "utf8");
  const queue = source.match(/^stack_queue_peak\(\) \{\n[\s\S]*?^\}/m)?.[0];
  assert.ok(queue);
  const { stdout } = await exec(
    "bash",
    [
      "-c",
      `
set -eu
run_id=fixture
P1_QUEUE_RATE=1
init_stack_files() { : > "$STRESS_RUN_DIR/lat.txt"; }
stack_lat_file() { echo "$STRESS_RUN_DIR/lat.txt"; }
stack_err_file() { echo "$STRESS_RUN_DIR/err.txt"; }
p1_duration_sec() { echo 1; }
p1_rate() { echo 1; }
run_rate_until() { local seq=0; while [ "$seq" -lt "$count" ]; do echo 10 >> "$7"; seq=$((seq+1)); done; }
wait_queue_label() { echo "$1"; }
sample_stack_response() { :; }
stack_stats() { :; }
record_anomaly() { exit 9; }
${queue}
count=45
stack_queue_peak
count=12
stack_queue_peak
`,
    ],
    { env: { ...process.env, STRESS_RUN_DIR: directory } },
  );
  assert.equal(stdout, "p1-q-fixture-25\np1-q-fixture-1\n");
  await rm(directory, { recursive: true });
});

test("peak rate sender runs each batch concurrently and waits for its samples", async () => {
  const directory = await fixture();
  const source = await readFile(join(root, "test/stress/stress-p1.sh"), "utf8");
  const sender = source.match(/^run_rate_until\(\) \{\n[\s\S]*?^\}/m)?.[0];
  assert.ok(sender);
  await exec(
    "bash",
    [
      "-c",
      `
set -eu
run_id=fixture
date() { if [ -f "$STRESS_RUN_DIR/complete" ]; then echo 101; else echo 100; fi; }
stress_request() {
  local marker
  marker=$(printf '%s' "$3" | jq -r .label)
  touch "$STRESS_RUN_DIR/started-$marker"
  while [ "$(find "$STRESS_RUN_DIR" -name 'started-*' | wc -l)" -lt 4 ]; do sleep .01; done
  printf '%s\n' "$3" >> "$STRESS_RUN_DIR/bodies.txt"
  echo 10 >> "$4"
  touch "$STRESS_RUN_DIR/complete"
}
${sender}
sleep 10 >/dev/null 2>&1 &
unrelated=$!
trap 'kill "$unrelated" 2>/dev/null || true' EXIT
run_rate_until fixture 4 1 /queue POST '{"label":"__RUN__-__SEQ__"}' "$STRESS_RUN_DIR/lat.txt" "$STRESS_RUN_DIR/err.txt"
`,
    ],
    { env: { ...process.env, STRESS_RUN_DIR: directory }, timeout: 5000 },
  );
  const bodies = (await readFile(join(directory, "bodies.txt"), "utf8"))
    .trim()
    .split("\n")
    .map((body) => JSON.parse(body).label)
    .sort();
  assert.deepEqual(bodies, [
    "fixture-1",
    "fixture-2",
    "fixture-3",
    "fixture-4",
  ]);
  assert.equal(
    (await readFile(join(directory, "lat.txt"), "utf8")).trim().split("\n")
      .length,
    4,
  );
  await assert.rejects(
    exec(
      "bash",
      [
        "-c",
        `
set -eu
run_id=fixture
date() { echo 100; }
stress_request() { return 7; }
${sender}
run_rate_until fixture 4 1 /queue POST '' "$STRESS_RUN_DIR/lat.txt" "$STRESS_RUN_DIR/err.txt"
`,
      ],
      { env: { ...process.env, STRESS_RUN_DIR: directory }, timeout: 5000 },
    ),
    { code: 1 },
  );
  await rm(directory, { recursive: true });
});

test("request latency uses curl transfer time and failed transfers remain errors", async () => {
  const directory = await fixture();
  await writeFile(
    join(directory, "curl"),
    '#!/bin/sh\nprintf "200 0.432100\\n"\nexit "${CURL_EXIT:-0}"\n',
    { mode: 0o755 },
  );
  const env = {
    ...process.env,
    PATH: `${directory}:${process.env.PATH}`,
    OPEN_COMPUTE_ROOT: root,
    STRESS_ACCOUNT_ID: "fixture-account",
    STRESS_RUN_DIR: directory,
  };
  const script = `set -eu; . '${join(root, "test/stress/common.sh")}'; now_ms() { exit 9; }; stress_request /queue POST '{}' "$STRESS_RUN_DIR/lat.txt" "$STRESS_RUN_DIR/err.txt"`;
  await exec("bash", ["-c", script], { env });
  await exec("bash", ["-c", script], { env: { ...env, CURL_EXIT: "56" } });
  assert.equal(
    await readFile(join(directory, "lat.txt"), "utf8"),
    "432\n432\n",
  );
  assert.equal(
    await readFile(join(directory, "err.txt"), "utf8"),
    "POST /queue 000\n",
  );
  await rm(directory, { recursive: true });
});
