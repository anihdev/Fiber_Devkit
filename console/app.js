const SCORE_FORMULA = "40% reachability + 30% channel readiness ratio + 30% peer presence";

const state = {
  taxonomy: [],
  activeCategory: "Route",
  predictionCodes: new Set(),
  lastRawPrediction: ""
};

const nodeGrid = document.getElementById("nodeGrid");
const refreshStatus = document.getElementById("refreshStatus");
const refreshTime = document.getElementById("refreshTime");
const connectionDot = document.getElementById("connectionDot");
const predictForm = document.getElementById("predictForm");
const predictionSummary = document.getElementById("predictionSummary");
const predictionTree = document.getElementById("predictionTree");
const predictionJson = document.getElementById("predictionJson");
const copyPrediction = document.getElementById("copyPrediction");
const taxonomyFilters = document.getElementById("taxonomyFilters");
const taxonomyList = document.getElementById("taxonomyList");
const lastRun = document.getElementById("lastRun");
const railLinks = Array.from(document.querySelectorAll("[data-section-link]"));
const sections = Array.from(document.querySelectorAll("[data-section]"));

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function display(value) {
  return value === null || value === undefined || value === "" ? "—" : value;
}

async function fetchJson(path) {
  const response = await fetch(path, { cache: "no-store" });
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(data.error || `request failed with HTTP ${response.status}`);
  }
  return data;
}

function activateRail(sectionId) {
  railLinks.forEach((link) => {
    link.classList.toggle("active", link.dataset.sectionLink === sectionId);
  });
}

function sectionIdFromHash() {
  const id = (location.hash || "").replace(/^#/, "");
  return sections.some((section) => section.id === id) ? id : "nodes";
}

function scrollToSection(sectionId) {
  document.getElementById(sectionId)?.scrollIntoView({ block: "start", behavior: "auto" });
}

function currentSectionId() {
  const headerOffset = window.matchMedia("(max-width: 720px)").matches ? 138 : 104;
  let current = sections[0]?.id || "nodes";
  sections.forEach((section) => {
    if (section.getBoundingClientRect().top <= headerOffset) {
      current = section.id;
    }
  });
  return current;
}

let railSyncFrame = 0;
let railLockUntil = 0;

function syncRailFromScroll() {
  railSyncFrame = 0;
  if (performance.now() < railLockUntil) {
    return;
  }
  activateRail(currentSectionId());
}

function scheduleRailSync() {
  if (railSyncFrame) {
    return;
  }
  railSyncFrame = window.requestAnimationFrame(syncRailFromScroll);
}

function navigateToSection(sectionId, updateHash = true) {
  railLockUntil = performance.now() + 1200;
  if (updateHash && location.hash !== `#${sectionId}`) {
    history.pushState(null, "", `#${sectionId}`);
  }
  activateRail(sectionId);
  scrollToSection(sectionId);
}

function setupRailNavigation() {
  railLinks.forEach((link) => {
    link.addEventListener("click", (event) => {
      event.preventDefault();
      navigateToSection(link.dataset.sectionLink);
    });
  });

  window.addEventListener("hashchange", () => navigateToSection(sectionIdFromHash(), false));
  window.addEventListener("popstate", () => navigateToSection(sectionIdFromHash(), false));
  window.addEventListener("scroll", scheduleRailSync, { passive: true });
  window.addEventListener("resize", scheduleRailSync);

  navigateToSection(sectionIdFromHash(), false);
}

function setConnectionStatus(success, message) {
  refreshStatus.classList.toggle("is-ok", success);
  refreshStatus.classList.toggle("is-fault", !success);
  connectionDot.className = `connection-dot ${success ? "ok" : "fault"}`;
  refreshTime.textContent = message;

  if (success) {
    connectionDot.classList.remove("pulse");
    void connectionDot.offsetWidth;
    connectionDot.classList.add("pulse");
  }
}

function healthScore(node) {
  const reachable = node.status === "reachable" ? 1 : 0;
  const readiness = node.totalChannelCount > 0
    ? node.readyChannelCount / node.totalChannelCount
    : 0;
  const peerPresence = (node.peerCount || 0) > 0 ? 1 : 0;
  return Math.round((0.4 * reachable + 0.3 * readiness + 0.3 * peerPresence) * 100);
}

function scoreTone(score) {
  if (score >= 85) return "good";
  if (score >= 50) return "warn";
  return "bad";
}

function scoreRing(score) {
  const radius = 20;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference - (score / 100) * circumference;
  return `
    <svg class="score-ring ${scoreTone(score)}" viewBox="0 0 52 52" role="img" aria-label="Client-side display score ${score} out of 100">
      <title>Client-side display score. Formula: ${SCORE_FORMULA}.</title>
      <circle class="score-ring-bg" cx="26" cy="26" r="${radius}"></circle>
      <circle class="score-ring-meter" cx="26" cy="26" r="${radius}" style="stroke-dasharray:${circumference};stroke-dashoffset:${offset};"></circle>
      <text x="26" y="30">${score}</text>
    </svg>
  `;
}

function renderChannels(node) {
  const channels = node.channels || [];
  const rows = channels.map((channel) => `
    <div class="channel-row">
      <strong>${escapeHtml(display(channel.peer))}</strong>
      <span>${escapeHtml(display(channel.state))}</span>
      <span>enabled ${escapeHtml(display(channel.enabled))}</span>
      <span>local ${escapeHtml(display(channel.localBalance))}</span>
      <span>remote ${escapeHtml(display(channel.remoteBalance))}</span>
    </div>
  `).join("");

  const empty = `
    <div class="empty-state small">
      No current channels reported by <code>list_channels</code>.
    </div>
  `;

  return `
    <details class="channel-details">
      <summary>${channels.length > 0 ? `Channels (${channels.length})` : "No current channels"}</summary>
      ${rows || empty}
    </details>
  `;
}

function nodeFailureDetails(node) {
  return [
    "Node RPC request failed.",
    `Endpoint: ${display(node.rpcEndpoint)}`,
    "",
    "What this usually means:",
    "- the Fiber container is stopped or still starting",
    "- the node exited before RPC became ready",
    "- the host RPC port is unavailable",
    "",
    "Try:",
    "- fiber up",
    "- fiber validate --live",
    "- fiber down && fiber up",
    "",
    `Raw error: ${display(node.error)}`
  ].join("\n");
}

function renderNodes(output) {
  const nodes = output.nodes || [];
  if (nodes.length === 0) {
    nodeGrid.innerHTML = '<div class="panel empty-state">No configured nodes found.</div>';
    return;
  }

  nodeGrid.innerHTML = nodes.map((node) => {
    const score = healthScore(node);
    const reachable = node.status === "reachable";
    const ready = display(node.readyChannelCount);
    const total = display(node.totalChannelCount);

    return `
      <article class="node-card ${reachable ? "reachable" : "unreachable"}">
        <div class="node-title">
          <div>
            <h3>${escapeHtml(node.name)}</h3>
            <p>${escapeHtml(display(node.rpcEndpoint))}</p>
          </div>
          <div class="node-status">
            <span class="status-dot ${reachable ? "ok" : "fault"}" aria-hidden="true"></span>
            <span class="status-chip ${reachable ? "ok" : "fault"}">${escapeHtml(display(node.status))}</span>
          </div>
        </div>

        <div class="node-body">
          ${scoreRing(score)}
          <div class="metric-grid">
            <div class="metric"><span>Pubkey</span><strong>${escapeHtml(display(node.shortPubkey))}</strong></div>
            <div class="metric"><span>Peers</span><strong>${escapeHtml(display(node.peerCount))}</strong></div>
            <div class="metric"><span>Ready</span><strong>${escapeHtml(ready)}</strong></div>
            <div class="metric"><span>Total channels</span><strong>${escapeHtml(total)}</strong></div>
          </div>
        </div>

        <p class="score-caption">Client-side display score, not diagnostic truth.</p>

        ${reachable ? renderChannels(node) : `
          <div class="unreachable-copy">
            DevKit could not reach this node's FNN RPC endpoint. Start the local network
            with <code>fiber up</code>, or run <code>fiber validate --live</code> to check RPC readiness.
          </div>
          <details class="technical-details">
            <summary>Technical details and next checks</summary>
            <pre>${escapeHtml(nodeFailureDetails(node))}</pre>
          </details>
        `}
      </article>
    `;
  }).join("");
}

async function refreshNodes() {
  try {
    const output = await fetchJson("/api/nodes");
    renderNodes(output);
    setConnectionStatus(true, new Date().toLocaleTimeString());
  } catch (error) {
    setConnectionStatus(false, `refresh failed ${new Date().toLocaleTimeString()}`);
    nodeGrid.innerHTML = `
      <div class="panel empty-state">
        Could not read configured nodes: ${escapeHtml(error.message)}
      </div>
    `;
  }
}

function predictionRoot(result) {
  return result.nativeFiber || result;
}

function renderPrimitive(value) {
  if (typeof value === "boolean") {
    return `<span class="json-primitive">${value ? "true" : "false"}</span>`;
  }
  if (typeof value === "number") {
    return `<span class="json-number">${escapeHtml(value)}</span>`;
  }
  if (value === null) {
    return '<span class="json-null">null</span>';
  }
  return `<span class="json-string">${escapeHtml(display(value))}</span>`;
}

function renderValue(value) {
  if (Array.isArray(value)) {
    if (value.length === 0) {
      return '<span class="json-empty">[]</span>';
    }
    return `
      <div class="json-list">
        ${value.map((item, index) => `
          <div class="json-row">
            <span class="json-key">${index}</span>
            <div>${renderValue(item)}</div>
          </div>
        `).join("")}
      </div>
    `;
  }

  if (value && typeof value === "object") {
    const entries = Object.entries(value);
    if (entries.length === 0) {
      return '<span class="json-empty">{}</span>';
    }
    return `
      <div class="json-object">
        ${entries.map(([key, nested]) => `
          <div class="json-row">
            <span class="json-key">${escapeHtml(key)}</span>
            <div>${renderValue(nested)}</div>
          </div>
        `).join("")}
      </div>
    `;
  }

  return renderPrimitive(value);
}

function summaryMetric(label, value) {
  return `
    <div class="metric">
      <span>${escapeHtml(label)}</span>
      <strong>${escapeHtml(display(value))}</strong>
    </div>
  `;
}

function renderPrediction(result) {
  const native = predictionRoot(result);
  const route = native.bestRoute;
  state.predictionCodes = new Set((native.warnings || []).map((warning) => warning.code));
  if (state.predictionCodes.size > 0) {
    state.activeCategory = "Prediction";
  }

  predictionSummary.innerHTML = [
    summaryMetric("Probability", native.probability ?? "n/a"),
    summaryMetric("Confidence", native.confidence || "unknown"),
    summaryMetric("Hop count", native.hopCount ?? 0),
    summaryMetric("Estimated fee", native.estimatedFee || "unknown"),
    summaryMetric("Capacity score", route?.capacityScore ?? "n/a"),
    summaryMetric("Hop penalty", route?.hopPenalty ?? "n/a"),
    summaryMetric("Channel health", route?.channelHealth ?? "n/a"),
    summaryMetric("Data source", route?.dataSource || "none")
  ].join("");

  if (result.cchBridged && result.cchBridged.available === false) {
    predictionSummary.insertAdjacentHTML("beforeend", `
      <div class="cch-note">
        <strong>CCH unavailable</strong>
        <span>${escapeHtml(result.cchBridged.reason)}</span>
      </div>
    `);
  }

  predictionTree.className = "prediction-tree";
  predictionTree.innerHTML = renderValue(result);
  state.lastRawPrediction = JSON.stringify(result, null, 2);
  predictionJson.textContent = state.lastRawPrediction;
  copyPrediction.disabled = false;
  renderTaxonomy();
}

function renderPredictionError(error) {
  const payload = { error: error.message };
  predictionSummary.innerHTML = "";
  predictionTree.className = "prediction-tree";
  predictionTree.innerHTML = renderValue(payload);
  state.lastRawPrediction = JSON.stringify(payload, null, 2);
  predictionJson.textContent = state.lastRawPrediction;
  copyPrediction.disabled = false;
}

predictForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(predictForm);
  const params = new URLSearchParams({
    from: formValue(data, "from"),
    to: formValue(data, "to"),
    amount: formValue(data, "amount"),
    asset: formValue(data, "asset") || "CKB"
  });
  if (data.get("cross_chain") === "on") {
    params.set("cross_chain", "true");
  }

  predictionSummary.innerHTML = "";
  predictionTree.className = "prediction-tree loading";
  predictionTree.textContent = "Analyzing route...";
  predictionJson.textContent = "Analyzing route...";
  copyPrediction.disabled = true;
  try {
    const result = await fetchJson(`/api/predict?${params.toString()}`);
    renderPrediction(result);
  } catch (error) {
    renderPredictionError(error);
  }
});

copyPrediction.addEventListener("click", async () => {
  if (!state.lastRawPrediction) {
    return;
  }

  try {
    await navigator.clipboard.writeText(state.lastRawPrediction);
    copyPrediction.textContent = "Copied";
  } catch {
    copyPrediction.textContent = "Copy failed";
  }

  window.setTimeout(() => {
    copyPrediction.textContent = "Copy raw JSON";
  }, 1400);
});

function formValue(data, name) {
  return String(data.get(name) || "").trim();
}

function categories() {
  const base = [...new Set(state.taxonomy.map((entry) => entry.category))];
  return state.predictionCodes.size > 0 ? ["Prediction", ...base] : base;
}

function renderTaxonomyFilters() {
  taxonomyFilters.innerHTML = categories().map((category) => `
    <button class="filter ${category === state.activeCategory ? "active" : ""}" data-category="${escapeHtml(category)}">${escapeHtml(category)}</button>
  `).join("");

  taxonomyFilters.querySelectorAll(".filter").forEach((button) => {
    button.addEventListener("click", () => {
      state.activeCategory = button.dataset.category;
      renderTaxonomy();
    });
  });
}

function severityClass(severity) {
  const normalized = String(severity || "").toLowerCase();
  if (normalized === "high") return "high";
  if (normalized === "medium") return "medium";
  return "low";
}

function renderTaxonomy() {
  renderTaxonomyFilters();
  const entries = state.activeCategory === "Prediction"
    ? state.taxonomy.filter((entry) => state.predictionCodes.has(entry.code))
    : state.taxonomy.filter((entry) => entry.category === state.activeCategory);

  taxonomyList.innerHTML = entries.slice(0, 6).map((entry) => `
    <article class="taxonomy-item">
      <div class="taxonomy-title">
        <h3>${escapeHtml(entry.code)}</h3>
        <span class="severity-chip ${severityClass(entry.severity)}">${escapeHtml(display(entry.severity))}</span>
      </div>
      <p class="taxonomy-sub">${escapeHtml(entry.subCategory)}</p>
      <p>${escapeHtml(entry.description)}</p>
      <p><strong>First fix:</strong> ${escapeHtml((entry.remediationSteps || [])[0] || "Inspect the raw result.")}</p>
    </article>
  `).join("") || '<p class="empty-state">No matching diagnostic hints for this filter.</p>';
}

async function loadTaxonomy() {
  try {
    state.taxonomy = await fetchJson("/api/taxonomy");
    renderTaxonomy();
  } catch (error) {
    taxonomyList.innerHTML = `<p class="empty-state">Could not load taxonomy: ${escapeHtml(error.message)}</p>`;
  }
}

async function loadLastRun() {
  try {
    const run = await fetchJson("/api/last-run");
    const steps = run.steps || [];
    const passed = steps.filter((step) => step.status === "passed").length;
    const failed = steps.length - passed;
    lastRun.className = `last-run-body ${run.passed ? "pass" : "fail"}`;
    lastRun.innerHTML = `
      <div class="last-run-title">
        <h3>${escapeHtml(display(run.scenario))}</h3>
        <span class="result-chip ${run.passed ? "pass" : "fail"}">${run.passed ? "PASS" : "FAIL"}</span>
      </div>
      <p>${passed} passed · ${failed} failed</p>
      <p>${escapeHtml(run.description || "No description provided.")}</p>
      <p>Report artifact: <code>.fiber/output/report.md</code></p>
    `;
  } catch {
    lastRun.className = "last-run-body empty";
    lastRun.innerHTML = `
      <p>No last run artifact available yet.</p>
      <p>Run <code>fiber run scenarios/network-smoke.yaml --report</code> to populate this panel.</p>
    `;
  }
}

setupRailNavigation();
refreshNodes();
loadTaxonomy();
loadLastRun();
setInterval(refreshNodes, 2500);
