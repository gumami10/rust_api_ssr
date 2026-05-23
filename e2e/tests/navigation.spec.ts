import { test, expect } from "@playwright/test";

test.describe("Navigation", () => {
  test("homepage loads and shows nav links", async ({ page }) => {
    await page.goto("/users");
    await expect(page).toHaveTitle(/Users/);

    // Nav links should be present
    await expect(page.locator("a:has-text('Users')")).toBeVisible();
    await expect(page.locator("a:has-text('Chat')")).toBeVisible();
    await expect(page.locator("a:has-text('Login')")).toBeVisible();
  });

  test("login page loads with form fields", async ({ page }) => {
    await page.goto("/login");
    await expect(page).toHaveTitle(/Login/);

    await expect(page.locator("#email")).toBeVisible();
    await expect(page.locator("#password")).toBeVisible();
    await expect(page.locator('button[type="submit"]')).toBeVisible();
    await expect(page.locator("a:has-text('Create account')")).toBeVisible();
  });

  test("sign up page loads with form fields", async ({ page }) => {
    await page.goto("/users/new");
    await expect(page).toHaveTitle(/New User/);

    await expect(page.locator("#name")).toBeVisible();
    await expect(page.locator("#email")).toBeVisible();
    await expect(page.locator("#password")).toBeVisible();
    await expect(page.locator('button[type="submit"]')).toBeVisible();
  });

  test("health check endpoint returns 200", async ({ request }) => {
    const response = await request.get("/healthz");
    expect(response.status()).toBe(200);
    const body = await response.json();
    expect(body.status).toBe("healthy");
  });

  test("clicking nav links navigates correctly", async ({ page }) => {
    await page.goto("/users");

    await page.click("a:has-text('Login')");
    await expect(page).toHaveURL("/login");

    await page.click("a:has-text('Create account')");
    await expect(page).toHaveURL("/users/new");

    await page.click("a:has-text('Users')");
    await expect(page).toHaveURL("/users");
  });
});
