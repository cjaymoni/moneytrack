import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderInApp } from "../test-utils";
import { SignIn } from "./SignIn";

describe("SignIn", () => {
  it("lets the person see the password they are typing, and hide it again", async () => {
    const user = userEvent.setup();
    renderInApp(<SignIn mode="login" />);
    const password = screen.getByLabelText("Password") as HTMLInputElement;
    expect(password.type).toBe("password");

    await user.click(screen.getByRole("checkbox", { name: "Show password" }));
    expect(password.type).toBe("text");

    await user.click(screen.getByRole("checkbox", { name: "Show password" }));
    expect(password.type).toBe("password");
  });
});
