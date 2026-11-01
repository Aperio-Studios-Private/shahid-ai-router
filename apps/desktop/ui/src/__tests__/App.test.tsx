import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import App from "../App";

describe("App shell", () => {
  it("renders the Phase 13 primary navigation (7 destinations + footer)", () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Main navigation" })).toBeInTheDocument();
    // Primary destinations (docs/08 amendment — folded nav).
    for (const label of [
      "Home",
      "Providers",
      "Routing",
      "Agent Integrations",
      "Local Gateway",
      "Activity",
      "Backup & Restore",
    ]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
    // Footer destinations.
    expect(screen.getByRole("button", { name: "About" })).toBeInTheDocument();
  });

  it("shows the guided Home screen by default", async () => {
    render(<App />);
    expect(
      screen.getByRole("heading", { name: "Welcome to Shahid AI Router" }),
    ).toBeInTheDocument();
    // The onboarding checklist renders with live-derived steps (async load).
    expect(await screen.findByText("Get started")).toBeInTheDocument();
    expect(await screen.findByText("Add a provider")).toBeInTheDocument();
  });
});
