import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import App from "../App";

describe("App shell", () => {
  it("renders the docs/08 navigation sections", () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Main navigation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Providers" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "API Keys" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Request Logs" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Backup & Restore" })).toBeInTheDocument();
  });

  it("shows the Overview screen by default", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Overview" })).toBeInTheDocument();
  });
});
