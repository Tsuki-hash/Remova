// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { AiSettingsPanel } from "../components/AiSettingsPanel";
import { api } from "../lib/api";
import { t } from "../i18n";
import type { AiConfigView } from "../types";

vi.mock("../lib/api", () => ({ api: { getAiConfig: vi.fn(), saveAiConfig: vi.fn() } }));
vi.mock("../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
const config: AiConfigView = { enabled: false, provider: "ollama", base_url: "http://localhost/v1",
  model: "saved-model", allow_cloud_paths: false, has_api_key: false };
beforeEach(() => vi.resetAllMocks());
afterEach(cleanup);

it("blocks edits and saves until the real configuration arrives", async () => {
  let resolve!: (value: AiConfigView) => void;
  vi.mocked(api.getAiConfig).mockReturnValue(new Promise(r => { resolve = r; }));
  const view = render(<AiSettingsPanel onClose={() => {}} />);
  expect(view.container.querySelector("fieldset")?.disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: t().aiSave }));
  expect(api.saveAiConfig).not.toHaveBeenCalled();
  await act(async () => resolve(config));
  expect(view.container.querySelector("fieldset")?.disabled).toBe(false);
  expect(screen.getByLabelText<HTMLInputElement>(t().aiModel).value).toBe("saved-model");
});

it("shows read failures, retries, and publishes enabled state only after successful save", async () => {
  vi.mocked(api.getAiConfig).mockRejectedValueOnce(new Error("read-denied")).mockResolvedValue(config);
  const onEnabledChange = vi.fn();
  render(<AiSettingsPanel onClose={() => {}} onEnabledChange={onEnabledChange} />);
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.getByRole<HTMLButtonElement>("button", { name: t().aiSave }).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: t().aiConfigRetry }));
  await waitFor(() => expect(screen.getByRole<HTMLButtonElement>("button", { name: t().aiSave }).disabled).toBe(false));
  fireEvent.click(screen.getByLabelText(t().aiEnable));
  vi.mocked(api.saveAiConfig).mockRejectedValueOnce(new Error("save-denied"));
  fireEvent.click(screen.getByRole("button", { name: t().aiSave }));
  await waitFor(() => expect(api.saveAiConfig).toHaveBeenCalledTimes(1));
  expect(onEnabledChange).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.getByRole<HTMLButtonElement>("button", { name: t().aiSave }).disabled).toBe(false));
  vi.mocked(api.saveAiConfig).mockResolvedValue({ ...config, enabled: true });
  fireEvent.click(screen.getByRole("button", { name: t().aiSave }));
  await waitFor(() => expect(onEnabledChange).toHaveBeenCalledWith(true));
});
