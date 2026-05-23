import { test, expect } from "@playwright/test";

test.describe("Authentication", () => {
  const uniqueEmail = () => `e2e_${Date.now()}@example.com`;

  test("sign up creates a user and redirects to profile", async ({ page }) => {
    await page.goto("/users/new");
    await expect(page).toHaveTitle(/New User/);

    const name = "E2E Test User";
    const email = uniqueEmail();
    const password = "SuperSecret123!";

    await page.fill("#name", name);
    await page.fill("#email", email);
    await page.fill("#password", password);
    await page.click('button[type="submit"]');

    // Sign-up auto-creates a session and redirects to the user profile
    await page.waitForURL(/\/users\/\d+/);
    await expect(page.locator("text=E2E Test User")).toBeVisible();
  });

  test("log in with valid credentials redirects to chat", async ({ page }) => {
    // 1. Create a user first via sign-up so we can log in
    const email = uniqueEmail();
    const password = "SuperSecret123!";

    await page.goto("/users/new");
    await page.fill("#name", "Login Test User");
    await page.fill("#email", email);
    await page.fill("#password", password);
    await page.click('button[type="submit"]');
    await page.waitForURL(/\/users\/\d+/);

    // 2. Log out
    await page.goto("/chat");
    await page.click('button:has-text("Logout")');
    await page.waitForURL("/users");

    // 3. Log in
    await page.goto("/login");
    await page.fill("#email", email);
    await page.fill("#password", password);
    await page.click('button[type="submit"]');

    await page.waitForURL("/chat");
    await expect(page.locator("text=Chat rooms")).toBeVisible();
  });

  test("log in with invalid credentials shows error", async ({ page }) => {
    await page.goto("/login");
    await page.fill("#email", "notfound@example.com");
    await page.fill("#password", "wrongpassword");
    await page.click('button[type="submit"]');

    // Stays on login page and shows error
    await expect(page).toHaveURL("/login");
    await expect(
      page.locator("text=Email or password is incorrect")
    ).toBeVisible();
  });

  test("log out clears session and shows login button", async ({ page }) => {
    // Create and log in
    const email = uniqueEmail();
    await page.goto("/users/new");
    await page.fill("#name", "Logout Test User");
    await page.fill("#email", email);
    await page.fill("#password", "SuperSecret123!");
    await page.click('button[type="submit"]');
    await page.waitForURL(/\/users\/\d+/);

    // Navigate to chat then log out
    await page.goto("/chat");
    await page.click('button:has-text("Logout")');
    await page.waitForURL("/users");

    // Login link should be visible again
    await expect(page.locator("a:has-text('Login')")).toBeVisible();
  });

  test("sign up with empty fields shows validation errors", async ({ page }) => {
    await page.goto("/users/new");
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL("/users"); // form posts to /users
    await expect(page.locator("text=Name is required")).toBeVisible();
    await expect(page.locator("text=Email is required")).toBeVisible();
    await expect(page.locator("text=Password is required")).toBeVisible();
  });

  test("log in with empty fields shows validation errors", async ({ page }) => {
    await page.goto("/login");
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL("/login");
    await expect(page.locator("text=Email is required")).toBeVisible();
    await expect(page.locator("text=Password is required")).toBeVisible();
  });

  test("duplicate email during sign up shows error", async ({ page }) => {
    const email = uniqueEmail();

    // First sign up
    await page.goto("/users/new");
    await page.fill("#name", "First User");
    await page.fill("#email", email);
    await page.fill("#password", "SuperSecret123!");
    await page.click('button[type="submit"]');
    await page.waitForURL(/\/users\/\d+/);

    // Second sign up with same email
    await page.goto("/users/new");
    await page.fill("#name", "Second User");
    await page.fill("#email", email);
    await page.fill("#password", "SuperSecret123!");
    await page.click('button[type="submit"]');

    await expect(page.locator("text=Email already exists")).toBeVisible();
  });
});
