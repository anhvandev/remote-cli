# E2E test theo kịch bản mà AI agent "nhìn thấy" app: các giải pháp hiện có (2026-11)

Ngày tra cứu: 2026-11-12. Mọi nguồn được đọc trong ngày này.
Nguồn chính:
- Release notes Playwright v1.53–v1.65 (GitHub Releases, `microsoft/playwright`). v1.65.0 phát hành 2026-11-05.
- Source Playwright tại tag `v1.65.0`: `packages/playwright/src/index.ts`, `packages/playwright/src/agents/*`, `packages/playwright/src/mcp/test/*`, `docs/src/*.md`.
- CLI `@playwright/test` 1.64.1 cài trong `dashboard/node_modules`. Đã chạy `playwright --help`, `trace --help`, `init-agents --help`.
- README `microsoft/playwright-mcp` (npm `@playwright/mcp` 0.0.85, 2026-11-09) và README `microsoft/playwright-cli` (npm `@playwright/cli` 0.1.24, 2026-11-10).
- docs.stagehand.dev (`/llms.txt`, các trang v4) và README `browserbase/stagehand` (npm 4.4.0, 2026-11-04).
- README và `MIGRATING.md` của `browser-use/browser-use` (PyPI 1.0.2, 2026-11-11).
- midscenejs.com (`/llms.txt`, các trang `.md`) cho `@midscene/web` 1.17.8 (2026-11-11).
- Repo `antiwork/shortest`, `magnitudedev/browser-agent`, `lucgagan/auto-playwright`, cùng npm registry cho `@zerostep/playwright`, `@octomind/octomind`, `@checksum-ai/runtime`, `momentic`, `@qawolf/cli`.
- momentic.ai/docs, docs.qawolf.com, platform.claude.com/docs, code.claude.com/docs.
- Repo này: `dashboard/playwright.config.ts`, `dashboard/tests/{devstack,live-view,fleet-filter,signin-fleet,app-shell,terminal}.spec.ts`, `crates/agent/src/platform.rs`.

Version và ngày lấy từ npm registry (`time[latest]`), PyPI, hoặc GitHub API (`pushed_at`, releases).
Mọi claim đều có nguồn đi kèm. Claim nào chưa xác nhận được thì ghi **UNVERIFIED**. Không chạy công cụ AI nào, không build gì.

---

## 0. Hiện trạng repo (để so độ phù hợp)

- `@playwright/test` `^1.64.1`, bản cài thực tế là 1.64.1 (`dashboard/package.json`). Config chỉ có `trace: "retain-on-failure"`: không bật screenshot, video, không khai báo reporter (`dashboard/playwright.config.ts`).
- Hai `webServer` dựng `rcli-devstack` (Server thật trên Postgres testcontainer, FakePlatform Agents, đồng hồ đóng băng) và `pnpm build && pnpm start` (`dashboard/playwright.config.ts`).
- Locator dùng `getByRole` và `getByLabel` với nhãn tiếng Việt ("Tên đăng nhập", "Xem trực tiếp", "Tìm Device") (`tests/devstack.ts`, `tests/live-view.spec.ts`, `tests/fleet-filter.spec.ts`).
- Những gì làm ảnh hưởng tính ổn định khi so ảnh hoặc snapshot:
  - Tên user là `user-${process.pid}-N` và hiện trong sidebar (`tests/devstack.ts`; `signin-fleet.spec.ts` kiểm `${user.name} · Operator`).
  - Các spec chạy song song và thay đổi fleet chung, nên số Device Offline thay đổi ("admin tests add/remove Offline Devices", `signin-fleet.spec.ts`).
  - Có vùng ổn định: "Phần cứng" luôn hiện `FakeOS 1.0`, `Fake CPU 9000 · 8 nhân`, … (`signin-fleet.spec.ts`).
- Live view: FakePlatform trả về khung hình **một màu** theo trạng thái session và input, có thể thêm chế độ noise (`crates/agent/src/platform.rs:309-330`). Vì vậy một model vision nhìn `<video>` chỉ thấy một khối màu. Các check tất định hiện có (`videoWidth`, fps qua `getVideoPlaybackQuality`) cho nhiều thông tin hơn (`tests/live-view.spec.ts`).
- Terminal: xterm render vào `.xterm-rows`, spec kiểm bằng `toContainText` (`tests/terminal.spec.ts`).

---

## 1. Playwright MCP (`@playwright/mcp`)

- **Version:** 0.0.85, phát hành 2026-11-09. Repo `microsoft/playwright-mcp` được push lần cuối 2026-11-11 (GitHub API). Từ Playwright 1.62, MCP server được gộp sẵn trong Playwright, chạy bằng `npx playwright mcp` (release notes v1.62.0). Bản 1.64.1 cài trong repo đã có lệnh `mcp` (`playwright --help`).
- **Agent thấy gì:**
  - Mặc định: "structured accessibility snapshots, bypassing the need for screenshots or visually-tuned models" (README).
  - `browser_snapshot` trả về aria snapshot có `ref`. Tham số: `target`, `depth`, `boxes`, `filename`. `browser_find` tìm text hoặc regex trong snapshot (README, mục Tools).
  - `browser_take_screenshot` có sẵn. README ghi: "Use browser_snapshot for actions by default".
- **Snapshot và vision:**
  - Không có "vision mode" thay cho snapshot. `--caps=vision` chỉ **thêm** các tool theo tọa độ: `browser_mouse_click_xy`, `browser_mouse_move_xy`, `browser_mouse_drag_xy`, `browser_mouse_down/up`, `browser_mouse_wheel` (README: "'vision': Coordinate-based interactions").
  - Các cap khác: `pdf`, `devtools` (tracing, video, `browser_annotate`, highlight), `network`, `storage`, `config`, `testing` (README).
- **Tool dùng cho pass/fail (`--caps=testing`):** `browser_verify_element_visible`, `browser_verify_list_visible`, `browser_verify_text_visible`, `browser_verify_value`, `browser_generate_locator` (README). LLM chọn gọi tool nào. Tool tự kiểm tất định trên trang.
- **Tool lõi:** `browser_navigate`, `browser_click`, `browser_type`, `browser_fill_form`, `browser_select_option`, `browser_evaluate`, `browser_console_messages`, `browser_network_requests`, `browser_wait_for`, `browser_tabs`, `browser_run_code_unsafe`, … Từ 1.64 có thêm `webmcp_<tool>` (README; release notes v1.64.0).
- **Artifact:**
  - `--output-dir` chứa screenshot và snapshot.
  - `--save-session` lưu phiên.
  - `browser_start_tracing/stop_tracing` và `browser_start_video/stop_video` (cap `devtools`).
  - `--codegen typescript` sinh code Playwright cho mỗi hành động (README).
- **Tính tất định, chi phí, tốc độ:**
  - Cách chọn phần tử theo `ref` là tất định ("Deterministic tool application", README). Quyết định của LLM thì không.
  - Chính README khuyên coding agent dùng `playwright-cli` thay MCP vì MCP "load large tool schemas and verbose accessibility trees into the model context".
  - Không có số liệu chi phí chính thức.
- **Độ phù hợp:** dùng để khám phá và debug tương tác với devstack đang chạy. MCP không phải test runner và không thay được 17 spec. `--storage-state` hoặc `--endpoint` cho phép gắn vào browser đã đăng nhập (README; `browser.bind()` từ v1.59).

## 2. Playwright Test Agents và các tính năng AI mới của Playwright

### 2a. planner / generator / healer
- **Nguồn gốc:** ra mắt ở v1.56.0 (2025-10-06). Lệnh `npx playwright init-agents --loop=…` (release notes v1.56.0). Bản 1.64.1 nhận các `--loop` sau: `claude`, `codex`, `copilot`, `opencode`, `vscode`, `vscode-legacy` (`init-agents --help`).
- **File tạo ra với `--loop=claude`:**
  - `.claude/agents/playwright-test-{planner,generator,healer}.md`.
  - `.mcp.json` trỏ tới `npx playwright run-test-mcp-server`.
  - Thư mục `specs/` và một seed test nếu chưa có (`packages/playwright/src/agents/generateAgents.ts` @v1.65.0).
- **Kịch bản:** planner khám phá app rồi viết plan Markdown vào `specs/`. Generator biến plan thành file `.spec.ts`, kiểm locator trên app thật. Healer chạy suite và sửa (https://playwright.dev/docs/test-agents).
- **Tool MCP riêng:** `test_list`, `test_run`, `test_debug`, `planner_setup_page`, `planner_save_plan`, `generator_setup_page`, `generator_read_log`, `generator_write_test`. Ngoài ra có các tool browser (`browser_snapshot`, `browser_generate_locator`, …) (`src/mcp/test/{testTools,plannerTools,generatorTools}.ts`; frontmatter `playwright-test-healer.agent.md`).
- **Pass/fail:** do assertion Playwright tất định trong code được sinh ra quyết định. Healer sửa cho tới khi pass. Nếu agent tin test đúng mà lỗi vẫn còn thì "mark this test as test.fixme()". Healer được dặn sửa cả "assertions and expected values" và "Do not ask user questions" (`playwright-test-healer.agent.md` @v1.65.0). Rủi ro: healer có thể nới assertion để test pass, nên cần review diff.
- **Model:** frontmatter healer ghi `model: sonnet` (cho Claude loop). Docs không bắt buộc model nào.

### 2b. Các tính năng AI khác (theo release notes)
- **v1.58 (2026-01-23):** `playwright-cli` kèm SKILLs, "token-efficient CLI mode". Repo `microsoft/playwright-cli` push 2026-11-11. Lệnh: `snapshot`, `screenshot`, `tracing-start`, `video-start`, `attach`, `show` (README playwright-cli).
- **v1.59 (2026-04-01):**
  - `page.screencast` (quay video, stream frame JPEG "for … AI vision", "Agentic video receipts").
  - `browser.bind()`.
  - `npx playwright test --debug=cli`.
  - **`npx playwright trace`** (CLI cho agent đọc trace).
  - `page.ariaSnapshot()` với `mode`, `depth`.
  - `locator.normalize()`.
- **v1.60 (2026-05-11):** `expect(page).toMatchAriaSnapshot()`, option `boxes` ("useful for AI consumption"), `testInfoError.errorContext` (aria snapshot của đối tượng tại lúc matcher fail).
- **v1.62 (2026-07-24):** gộp MCP và CLI vào gói Playwright (`npx playwright mcp`, `npx playwright cli`).
- **v1.63 (2026-09-04):** `trace.snapshots: { dom, aria, screen }` cùng chế độ **Display Aria** trong trace viewer, `ariaSnapshotJSON()`, step `params/subtitle`.
- **v1.64 (2026-10-07):** `page.webmcp`, `page.getByRef()` (nhận ref `e2` từ aria snapshot ở mode `'ai'`), video VP9 và option `fps`.
- **v1.65 (2026-11-05):** reporter `failures`, option `onlyFailures` cho HTML/JSON/JUnit. Text không được render bị loại khỏi aria snapshot.
- **Mặc định đã có, kể cả 1.64.1:** khi test fail, Playwright ghi `error-context.md` vào thư mục output của test. File chứa lỗi kèm aria snapshot của trang (`page.ariaSnapshot({ mode: 'ai' })`) và được đính kèm vào kết quả test. Biến `PLAYWRIGHT_NO_COPY_PROMPT` tắt tính năng này (`packages/playwright/src/index.ts:766-837` @v1.65.0; chuỗi `error-context` có trong `playwright@1.64.1/lib/index.js` đã cài).
- **Đã kiểm trên 1.64.1:** `npx playwright trace` có `open`, `actions`, `action`, `snapshot`, `screenshot <action-id>`, `console`, `errors`, `requests`, `attachments`, `install-skill` (`trace --help`).

## 3. `toMatchAriaSnapshot` / `ariaSnapshot()`

- **Kịch bản:** template YAML của cây accessibility, inline hoặc file `.aria.yml` qua `{ name: 'x.aria.yml' }`. Template hỗ trợ partial match (bỏ attribute hoặc tên), `/children: contain|equal|deep-equal`, và regex trong tên (`docs/src/aria-snapshots.md` @v1.65.0).
- **Agent thấy gì:** role, accessible name, state (checked, disabled, level, …). Mode `'ai'` thêm `[ref=eN]`, gồm cả iframe, và không chờ phần tử (`docs/src/api/class-locator.md`, `ariaSnapshot.mode` từ v1.59).
- **Pass/fail:** so khớp cấu trúc tất định, không dùng LLM. Cập nhật baseline bằng `--update-snapshots`. Có thể ghi patch 3-way bằng `--update-source-method=3way` (aria-snapshots.md).
- **Chi phí:** như một assertion DOM bình thường.
- **Artifact:** diff trong lỗi, `error-context.md`, file `.aria.yml`.
- **Độ phù hợp: cao.** Repo đã dùng `getByRole`/`getByLabel`, nên các accessible name tiếng Việt chính là thứ snapshot ghi lại. Snapshot phải scope vào vùng ổn định (ví dụ region "Phần cứng"), hoặc dùng regex cho tên user `user-\d+-\d+` và cho số Device.
- **Lưu ý nâng cấp:** v1.65 đổi cách tính accessible name đối với text không render (release notes v1.65.0).

## 4. `toHaveScreenshot` và các reporter/trace dùng làm artifact

- **`toHaveScreenshot`:**
  - So pixel với baseline. Có `maxDiffPixels`, `threshold`, `stylePath`, `mask` (`docs/src/test-snapshots-js.md`). Hỗ trợ lưu WebP từ v1.62/v1.64.
  - Docs cảnh báo: "Browser rendering can vary based on the host OS, version, settings, hardware…". Baseline được đặt tên theo `chromium-darwin` và các biến thể tương tự.
  - Tất định chỉ khi chạy trên cùng môi trường. Mac dev và Linux CI cần hai bộ baseline riêng.
- **Reporter và artifact agent có thể đọc:**
  - `json` (`outputFile`, `PLAYWRIGHT_JSON_OUTPUT_FILE`): cấu trúc kết quả, lỗi, đường dẫn attachment.
  - `html` (có nút "Copy prompt", tắt bằng `noCopyPrompt`).
  - `blob` (dùng để merge shard), `junit`, `perfetto` (v1.63), `failures` (v1.65) (`docs/src/test-reporters-js.md` @v1.65.0).
  - `onlyFailures` cần v1.65. Repo đang ở 1.64.1.
- **Trace (`trace.zip`):**
  - DOM snapshot trước và sau từng action, screencast frame, network, console.
  - Từ v1.63 có thể thêm aria và screen snapshot cho mỗi action.
  - Agent đọc trace qua `npx playwright trace …` mà không cần GUI (release notes v1.59, v1.63).
- **`screenshot: 'only-on-failure'`, `video: 'retain-on-failure'`:** test options có sẵn (`docs/src/test-use-options-js.md`; video modes mở rộng ở v1.61).

## 5. Stagehand (Browserbase)

- **Version:** 4.4.0 (2026-11-04). v4.0.0 ra 2026-08-10. Tag `v3-latest` là 3.7.3 (2026-08-28) (npm). Repo push 2026-11-12.
- **Kịch bản:** code TS, Python hoặc Go. Có `act("click the sign in button")`, `extract(instruction, zodSchema)`, `observe(...)` trả về selector thật (README).
- **`agent()` đã bị bỏ trong v4:** "**`agent()` is gone.** Nothing in v4 replaces it one-for-one". Thay bằng "code mode" hoặc vòng tool-calling tự viết. `agent({ mode: "cua" })`: "No equivalent" (https://docs.stagehand.dev/v4/migrations/v3.md). v3 vẫn có `agent()` (docs v3).
- **Agent thấy gì:** "Hybrid accessibility-tree trimming" (README).
- **Pass/fail:** Stagehand không có runner và không có `expect`. Docs ghi rõ không có "fixtures, no `expect()`, no retries, no HTML reporter, no `codegen`, and no trace viewer". Bạn tự assert trên kết quả `extract()` (https://docs.stagehand.dev/v4/migrations/playwright.md).
- **Cache:**
  - v4 chỉ có cache **phía server Browserbase**. Với local browser, "the `cache` option has no effect and every call runs inference" (https://docs.stagehand.dev/v4/best-practices/caching.md).
  - `enableCaching`/`cacheDir` (cache local của v3) đã bị bỏ (migrations/v3).
  - Khi cache hit thì không gọi LLM. `metadata.cache.status` có giá trị `HIT`, `MISS`, `DISABLED`.
- **Artifact:** `{ data, metadata }` (action ID, cache status, token usage), `stagehand.metrics()`, OTel traces (migrations/v3; README).
- **Độ phù hợp: thấp.** "Stagehand v4 drives Chromium over the Chrome DevTools Protocol and has no Playwright dependency, so you can't hand a Playwright `Page` to `act()`. There is no interop" (migrations/playwright). Dùng v4 thì phải port spec. `localBrowser.connect({ cdpUrl })` có thể gắn vào browser đang chạy, nhưng việc dùng chung với Playwright trong cùng test là **UNVERIFIED**. v3 có trang "Use Stagehand with Playwright" (docs v3 index). Chi tiết chưa đọc.

## 6. browser-use

- **Version:** PyPI 1.0.0 (2026-11-10), 1.0.2 (2026-11-11), 1.0.3rc1 (2026-11-12). README khuyên "pin it exactly … releases move fast and can break APIs".
- **Kịch bản:** `Agent('task', llm=…, browser=Browser('chromium')).run_sync()`, viết bằng Python. Từ 1.0, model viết code Python chạy trong REPL dựa trên raw CDP (README).
- **Agent thấy gì:** screenshot khi agent tự gọi. `mode='ultrafast'` "acts on accessibility-tree ids". `use_vision=False` chặn không cho screenshot tới model (README; MIGRATING.md).
- **Pass/fail:** `result.success`, nghĩa là "the agent's own verdict", "the agent's own claim". **Judge đã bị gỡ ở 1.0:** "`use_judge` (ignored), `judge_llm`, `ground_truth` … removed" (MIGRATING.md).
- **Artifact:** `result.messages` (transcript), `status`, `usage`, `cost_usd`, file trong `agent.workspace`. 1.0 đã **bỏ** `generate_gif`, `record_video_dir`, `traces_dir`, `save_history()`/`rerun_history()`, `save_as_playwright_script()` (MIGRATING.md).
- **Độ phù hợp: thấp.** Thư viện là Python, không gắn vào `@playwright/test`. Thiếu replay và trace, và kết quả là tự agent đánh giá.

## 7. Midscene.js

- **Version:** `@midscene/web` 1.17.8 (2026-11-11). Repo push 2026-11-12.
- **Kịch bản:**
  - Gắn vào spec Playwright có sẵn qua `PlaywrightAiFixture()`. Các method: `aiAct`, `aiTap`, `aiInput`, `aiQuery`, `aiBoolean/aiNumber/aiString`, `aiAssert`, `aiWaitFor`. `page` và `expect` của Playwright vẫn dùng chung trong cùng test (https://midscenejs.com/integrate-with-playwright.md).
  - Có thêm YAML runner và `midscene-test`. Gherkin đang Beta từ 1.10 (llms.txt).
- **Agent thấy gì:** screenshot qua "multimodal vision models … operating directly on the rendered UI" (platforms/index). `aiAssert`/`aiQuery` có option `screenshotIncluded` (mặc định `true`) và `domIncluded` (mặc định `false`, hoặc `'visible-only'`) (https://midscenejs.com/reference/index.md).
- **Pass/fail:** `aiAssert` throw lỗi kèm "the detailed reason provided by the AI model". Docs khuyên với check cần chính xác thì dùng `aiQuery()` rồi `expect` thường (reference).
- **Cache:** cache plan (`aiAct`) và XPath (web) trong `./midscene_run/cache/*.cache.yaml`. "`aiBoolean`, `aiQuery`, `aiAssert` will never be cached", nên mỗi lần chạy assertion đều gọi model (https://midscenejs.com/caching.md).
- **Model hỗ trợ:** Doubao Seed, Qwen, DeepSeek, Gemini, OpenAI GPT, Kimi, MiMo, GLM-V, Ant Ling, UI-TARS, AutoGLM. Anthropic Claude **không có** trong danh sách (https://midscenejs.com/model-selection.md).
- **Artifact:**
  - Reporter `@midscene/web/playwright-reporter` xuất HTML có screenshot từng bước, ở dạng `single-html` hoặc `html-and-external-assets`, vào `midscene_run/report/`.
  - `report-tool --action split|to-markdown` (từ v1.7.0) tách JSON, screenshot và Markdown cho agent khác đọc (https://midscenejs.com/consume-report-file.md).
- **MCP đã bị gỡ:** "Midscene no longer ships MCP servers". Bản cuối còn MCP là 1.9.8 (llms.txt, trang mcp).
- **Độ phù hợp: trung bình–cao** cho một bước LLM-judge gắn vào spec sẵn có. Hạn chế: cần model vision ngoài danh sách Claude, `timeout` 30 s hiện tại phải tăng (docs dùng 90 s), mỗi `aiAssert` tốn một lần gọi model.

## 8. Shortest (antiwork)

- **Version:** npm `@antiwork/shortest` 0.4.9 (2025-04-01), không có bản mới hơn. Commit gần nhất 2026-05-21 ("Remove .cursorrules"). Commit có nội dung gần nhất là 2025-08-18 (GitHub API). **Gần như ngừng phát triển.**
- **Kịch bản:** `shortest("Login to the app using email and password", {...}).after(async ({ page }) => …)` trong `*.test.ts`. Runner riêng `pnpm shortest`, dựng trên Playwright (README).
- **Agent thấy gì:** Anthropic computer-use tool (`computer_20241022`, `computer_20250124`) cùng bash tool (`packages/shortest/src/ai/tools/anthropic/*`).
- **Pass/fail:** model trả về JSON `{ status: "passed" | "failed", reason }` (`src/ai/prompts/index.ts`). Có cache trong `.shortest/` (README).
- **Rủi ro version:** theo docs Anthropic hiện tại, các model Claude 5.x chỉ nhận `computer_toolset_20260801` và "return a 400 error for any computer use tool version they don't accept" (computer-use-tool docs). Suy ra Shortest chỉ chạy được với model cũ. Chưa thử thực tế: **UNVERIFIED**.
- **Độ phù hợp: thấp.** Runner riêng, không gắn được vào spec hiện có, và đã cũ.

## 9. Các công cụ khác: kịch bản ngôn ngữ tự nhiên với LLM làm giám khảo

- **Magnitude:**
  - Repo `magnitudedev/magnitude` nay là "inference engine" và không còn là browser testing (README, 2026-11-12). Code test chuyển sang `magnitudedev/browser-agent`, push lần cuối 2026-02-08. npm `magnitude-test` 0.3.13 (2026-02-08).
  - Thiết kế "vision-first". `agent.check()` hỏi LLM với "a screenshot taken afterwards" và throw `Check failed` (`packages/magnitude-test/src/agent/index.ts`). README khuyên "Claude Sonnet 4".
  - Có runner riêng và có trang `docs/core-concepts/playwright.mdx` (chưa đọc). **Ngừng hoạt động 9 tháng.**
- **ZeroStep:**
  - `@zerostep/playwright` 0.1.5 (2023-12-08). Hàm `ai(prompt, { page, test })` dùng bên trong test Playwright, có `type: 'action' | 'assert' | 'query'`. `model` chỉ hỗ trợ `'GPT_3.5'`. Cần `ZEROSTEP_TOKEN` (README trên npm).
  - Repo GitHub `zerostep-ai/zerostep` trả 404. zerostep.com không kết nối được ngày 2026-11-12. Việc dịch vụ đã đóng là **UNVERIFIED**, nhưng nên coi như đã chết.
- **auto-playwright:**
  - 1.16.1 (2025-05-01). Repo push lần cuối 2025-07-08. `auto("Is … equal to …?", { page, test })` trả về boolean rồi `expect` thường.
  - Gửi HTML đã sanitize (`sanitize-html`) tới OpenAI, không gửi screenshot. README tự ước tính khoảng $0.01/bước với GPT-4 Turbo, ở thời điểm viết README (README).
- **Momentic (SaaS):**
  - CLI `momentic` 3.72.1 (2026-11-09). Test viết bằng YAML trong repo, gồm preset step và AI step.
  - "Momentic's AI evaluates assertions against the DOM, accessibility tree, and a screenshot of the viewport" (https://momentic.ai/docs/core-concepts/writing-assertions.md).
  - Có step cache (llms.txt). Report local dạng JUnit, Allure, **Playwright JSON**. Dashboard có video, trace (DOM, network, console), AI reasoning cho từng step (https://momentic.ai/docs/running-tests/results.md).
  - Cần `MOMENTIC_API_KEY`. Thay thế spec hiện có, không gắn vào spec.
- **QA Wolf (dịch vụ có người và agent):**
  - `@qawolf/cli` 2.11.0 (2026-11-10). Agent của QA Wolf sinh flow **Playwright** cho web (https://docs.qawolf.com/test-automation.md).
  - Pass/fail là assertion Playwright trong flow sinh ra, có "soft check". CLI chạy flow local từ `.qawolf/<env>`, nhưng việc tạo flow cần nền tảng trả phí (README `qawolf/cli`). Có MCP cho coding agent (llms.txt).
- **Checksum:**
  - `@checksum-ai/runtime` 5.5.3 (2026-11-12). Test là Playwright do Checksum sinh ra (`.checksum.spec.ts` hoặc `.checksum.md`).
  - Khi chạy có API key, `checksumAI("desc", () => …)` cho phép model "regenerate the failed section … without changing the assertions". Cấu hình `useChecksumAI: { actions, assertions, visualComparison }` (README trên npm).
  - AI ở đây chủ yếu dùng để tự sửa, không phải để chấm điểm.
- **Octomind:**
  - `@octomind/octomind` 4.7.0 (2026-02-03). Mọi repo GitHub của OctoMind-dev đều đã **archived** (cli 2026-04-24, octomind-mcp 2026-07-01, mintlify-docs 2026-07-14). octomind.dev không kết nối được ngày 2026-11-12.
  - Trạng thái sản phẩm là **UNVERIFIED** (có thể đã ngừng). Không nên chọn.
- **mabl:**
  - "Visual assertions" (trước đây gọi là GenAI Assertions) đánh giá bằng **screenshot của element, không dùng DOM**. Mabl tự sinh "assertion criteria" lần đầu để chấm nhất quán. Tối đa 30 assertion mỗi test. Chạy local bằng CLI cần `--allow-billable-features`.
  - Nguồn: help.mabl.com (articles 31576174565268, 28810650854292). Trang trả 403 khi fetch trực tiếp, nội dung lấy qua chỉ mục tìm kiếm, nên **UNVERIFIED một phần**.
- **Testim (Tricentis):** Copilot sinh JS cho custom step. Custom validation trả boolean (help.testim.io/docs/coding-assistant, /docs/custom-code; lấy qua chỉ mục tìm kiếm). Không tìm thấy step assertion bằng ngôn ngữ tự nhiên chấm bởi LLM: **UNVERIFIED**.
- **Chrome DevTools MCP (bổ sung):**
  - `chrome-devtools-mcp` 1.13.0 (2026-11-11). Tool: `take_snapshot`, `take_screenshot`, `performance_start_trace/stop_trace`, `lighthouse_audit`, `list_console_messages`, `screencast_start` (`docs/tool-reference.md`).
  - Mạnh về debug và hiệu năng. Không có assertion.

## 10. Claude computer use / browser use và Claude Code điều khiển browser

- **Claude API, browser use tool:**
  - `browser_toolset_20260801`, trạng thái stable, có 27 member tool mặc định như `navigate`, `read_page`, `find`, `left_click`, `screenshot`.
  - Claude làm việc với trang "through its structure (the accessibility tree, elements, forms, and tabs) and through screenshots". `read_page` trả về text kèm ref.
  - Ứng dụng của bạn tự thực thi mọi call (https://platform.claude.com/docs/en/agents-and-tools/tool-use/browser-use-tool).
  - SDK Python/TS có class toolset (beta), "write your own [driver] with … Playwright" (browser-use-sdk docs).
- **Claude API, computer use:**
  - `computer_toolset_20260801` gồm 17 member tool (`screenshot`, `left_click`, `type`, `zoom`, …). Chỉ dùng screenshot và tọa độ.
  - Model 5.x từ chối các version tool cũ. Opus 4.5–4.7 và Sonnet 4.6 chỉ dùng `computer_20251124` kèm beta header (computer-use-tool docs).
- **Pass/fail:** không có sẵn. Bạn phải tự đặt điều kiện dừng và hỏi model một verdict có cấu trúc.
- **Claude Code + Chrome:**
  - `claude --chrome` dùng extension "Claude in Chrome" và chạy trong cửa sổ Chrome thật, hiển thị được. Có ghi GIF.
  - **Bắt buộc đăng nhập `/login`.** Với API key hoặc `claude setup-token`, Claude Code giữ Chrome integration tắt. Không có qua Bedrock, Vertex hay Foundry (https://code.claude.com/docs/en/chrome.md). Vì vậy cách này không dùng được trong CI headless.
- **Claude Code + Playwright MCP/CLI:** `claude mcp add playwright npx @playwright/mcp@latest` (README playwright-mcp), hoặc `playwright-cli install --skills` (README playwright-cli). Claude Code bản mới nhất là 2.1.322 (npm, 2026-11-10).

---

## So sánh

| Giải pháp | Cách viết kịch bản | Agent thấy gì | Pass/fail | Tất định / chi phí | Trạng thái 2026-11 | Artifact | Gắn vào spec hiện có? |
|---|---|---|---|---|---|---|---|
| Playwright MCP | prompt tương tác | aria snapshot + ref; screenshot; tool tọa độ nếu bật `--caps=vision` | LLM gọi `browser_verify_*` (`--caps=testing`) | phần chọn phần tử tất định, LLM thì không; tốn token | 0.0.85, 2026-11-09; đã gộp vào Playwright | screenshot/snapshot trong `--output-dir`, trace, video, codegen TS | Không (công cụ khám phá) |
| Playwright Test Agents | plan Markdown → `.spec.ts` | aria snapshot qua MCP | assertion Playwright trong code sinh ra | runtime tất định; chỉ tốn LLM lúc sinh và sửa | có từ v1.56; 1.65.0 | spec, plan `specs/*.md`, diff của healer | Có (sinh spec cùng kiểu) |
| `toMatchAriaSnapshot` | YAML (partial/regex) | cây a11y | so khớp cấu trúc | tất định, không tốn tiền | 1.65.0 | diff, `.aria.yml`, `error-context.md` | Có |
| `toHaveScreenshot` + trace/reporters | code | pixel / DOM, aria, screen snapshot | so pixel | tất định trên cùng OS | 1.65.0 | PNG/WebP diff, `trace.zip`, JSON/HTML/blob, `error-context.md` | Có |
| Stagehand v4 | code + `act/extract/observe` | a11y tree đã cắt gọn | bạn tự assert trên `extract()` | cache chỉ trên Browserbase; local luôn gọi LLM | 4.4.0, 2026-11-04; bỏ `agent()` | metadata, metrics, OTel | Không (không interop với Playwright) |
| browser-use 1.0 | task Python | screenshot, a11y id | `result.success` do agent tự khai | không tất định | 1.0.2, 2026-11-11 | transcript, workspace; bỏ gif/video/trace | Không |
| Midscene | `aiAct/aiAssert/aiQuery` trong spec, hoặc YAML | screenshot (+DOM tùy chọn) | VLM trả lý do; throw khi sai | assertion không cache; cần VLM | 1.17.8, 2026-11-11 | HTML report, `report-tool` → JSON/Markdown + screenshot | Có (`PlaywrightAiFixture`) |
| Shortest | `shortest("…")` | computer-use screenshot | JSON `{status, reason}` | không tất định; Claude | 0.4.9, 2025-04 (ngừng) | cache `.shortest/` | Không |
| Magnitude | `.mag.ts`, `agent.check()` | screenshot | LLM chấm | không tất định | 0.3.13, 2026-02 (ngừng) | log của runner | Không |
| ZeroStep / auto-playwright | `ai()` / `auto()` trong spec | DOM/HTML | boolean → `expect` | không tất định | 2023-12 / 2025-05 (ngừng) | không có gì riêng | Có, nhưng đã chết hoặc cũ |
| Momentic | YAML | DOM + a11y + screenshot | AI assertion | có step cache; SaaS | CLI 3.72.1, 2026-11-09 | Playwright JSON/JUnit/Allure; dashboard video/trace | Không (thay thế) |
| QA Wolf / Checksum | agent hoặc dịch vụ sinh Playwright | — | assertion Playwright | runtime tất định; trả phí | 2.11.0 / 5.5.3, 2026-11 | Playwright | Một phần (output là Playwright) |
| Claude browser/computer use | prompt + loop tự viết | a11y + screenshot / chỉ screenshot | tự thiết kế | không tất định; theo token | toolset `_20260801` | tùy bạn ghi | Có, nếu tự viết helper dùng `page` |
| Claude Code + Chrome | chat | trang thật, console, DOM | Claude kết luận | không tất định; cần đăng nhập claude.ai | Claude Code 2.1.322 | GIF | Không (chỉ tương tác thủ công) |

---

## Phù hợp với repo này

Xếp hạng theo độ phù hợp và chi phí cho stack `@playwright/test` + `rcli-devstack`.

### 1. Làm artifact Playwright sẵn có thành thứ agent đọc được, rồi thêm aria snapshot (không gọi LLM trong CI)
- **Lý do đứng đầu:**
  - Repo đã có trace khi fail, và `error-context.md` kèm aria snapshot mode `'ai'` đã được ghi mặc định từ 1.64.1 (mục 2b).
  - Claude Code đọc được thứ đó qua `npx playwright trace …` mà không cần thêm package nào.
  - `toMatchAriaSnapshot` khớp với cách repo đã chọn locator theo role và nhãn tiếng Việt.
  - FakePlatform cho giá trị cố định ở region "Phần cứng".
- **Bước nhỏ nhất:**
  - Thêm một `await expect(page.getByRole("region", { name: "Phần cứng" })).toMatchAriaSnapshot(...)` vào `dashboard/tests/signin-fleet.spec.ts`, sinh baseline bằng `--update-snapshots`.
  - Trong `playwright.config.ts`, đặt `trace: { mode: "retain-on-failure", snapshots: { dom: true, aria: true, screen: true } }` (v1.63+). Ví dụ trong release notes dùng `mode: 'on'`, còn việc kết hợp với `retain-on-failure` thì **UNVERIFIED**.
- **Đánh đổi:**
  - Aria snapshot không bắt được lỗi thuần hình ảnh (màu, chồng lấn, layout).
  - Không nên snapshot toàn trang, vì tên user chứa pid và số Device thay đổi do các spec chạy song song. Phải scope vào region hoặc dùng regex.
  - Khi lên 1.65 có thể thay đổi accessible name (text không render). Lúc đó mới dùng được `onlyFailures` hoặc reporter `failures`.

### 2. Thêm một bước LLM-judge tùy chọn, đọc screenshot và aria snapshot của trang
- **Hai cách:**
  - **(a) Midscene `aiAssert`** qua `PlaywrightAiFixture`. Gắn thẳng vào spec, có report HTML, và `report-tool` xuất Markdown kèm screenshot.
  - **(b) Helper khoảng 30 dòng** gọi Claude Messages API với `page.screenshot()` và `page.ariaSnapshot({ mode: "ai" })`. Helper đòi về JSON `{ pass, reason }`, gắn vào báo cáo bằng `testInfo.attach`, rồi `expect(pass)`.
- **Bước nhỏ nhất:** một assertion sau `signIn` trong `dashboard/tests/app-shell.spec.ts`, ví dụ "sidebar đánh dấu mục Device đang chọn và không có mục Quản trị". Chỉ chạy khi `RCLI_AI_JUDGE=1`, để suite mặc định vẫn tất định và không tốn tiền.
- **Đánh đổi:**
  - Không tất định. Mỗi lần gọi đều tốn model, vì Midscene không bao giờ cache `aiAssert`.
  - Phải tăng `timeout` (hiện 30 s).
  - Midscene không liệt kê Claude, nên cách (a) kéo theo nhà cung cấp model khác.
  - Không nên áp dụng cho `live-view.spec.ts`: khung hình FakePlatform là một màu, nên check `videoWidth`/fps hiện tại đã mạnh hơn.
  - Chỉ đáng dùng cho check kiểu cảm quan, như layout hay thông báo lỗi có "hợp lý" không. Không thay assertion chính xác.

### 3. Playwright Test Agents / MCP / CLI để khám phá và sửa spec (không làm cổng CI)
- **Bước nhỏ nhất:** chạy `npx playwright init-agents --loop=claude` trong `dashboard/`. Lệnh tạo `.claude/agents/*`, `.mcp.json` và seed test. Sửa seed test để gọi `newUser()` + `signIn()` từ `tests/devstack.ts`. `webServer` trong config đã tự dựng devstack. Sau đó cho planner viết plan cho một luồng chưa có spec.
- **Đánh đổi:**
  - Output là spec Playwright tất định, nên không đổi cách chạy CI.
  - Healer được dặn sửa cả expected value và có thể đánh `test.fixme()`, nên bắt buộc review diff.
  - Lệnh ghi đè file `.claude/` và `.mcp.json` ở cấp `dashboard/`. Cần kiểm xung đột với cấu hình Claude Code hiện có (**UNVERIFIED**, chưa chạy).

### Không khuyến nghị cho repo này
- Stagehand v4: không interop với Playwright, cache chỉ có trên Browserbase.
- browser-use 1.0: Python, đã gỡ judge và trace.
- Shortest, Magnitude, ZeroStep, auto-playwright: ngừng phát triển hoặc cũ.
- Octomind: repo đã archived.
- Momentic, mabl, Testim: SaaS thay thế cả stack và cần app reachable từ dịch vụ, trong khi devstack chỉ chạy trên `127.0.0.1`.
- Claude Code + Chrome: cần đăng nhập claude.ai, không dùng được trong CI.
