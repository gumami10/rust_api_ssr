import { test, expect } from "@playwright/test";

const uniqueEmail = () => `test-${Date.now()}-${Math.random().toString(36).slice(2)}@example.com`;

test.describe("Sign up", () => {
  test("creates a new user and redirects to profile", async ({ page }) => {
    const email = uniqueEmail();
    const name = "Test User";
    const password = "securepassword123";

    await page.goto("/users/new");
    await expect(page.locator("h1")).toContainText("New User");

    await page.fill("#name", name);
    await page.fill("#email", email);
    await page.fill("#password", password);
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL(/\/users\/\d+/);
    await expect(page.locator("body")).toContainText(name);
  });

  test("shows validation errors for empty fields", async ({ page }) => {
    await page.goto("/users/new");
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL(/\/users/);
  });

  test("rejects short passwords", async ({ page }) => {
    await page.goto("/users/new");
    await page.fill("#name", "Short Pass");
    await page.fill("#email", uniqueEmail());
    await page.fill("#password", "short");
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL(/\/users/);
  });

  test("rejects duplicate email", async ({ browser }) => {
    const email = uniqueEmail();

    const ctx1 = await browser.newContext();
    const page1 = await ctx1.newPage();
    await page1.goto("/users/new");
    await page1.fill("#name", "User One");
    await page1.fill("#email", email);
    await page1.fill("#password", "password123");
    await page1.click('button[type="submit"]');
    await expect(page1).toHaveURL(/\/users\/\d+/);
    await ctx1.close();

    const ctx2 = await browser.newContext();
    const page2 = await ctx2.newPage();
    await page2.goto("/users/new");
    await page2.fill("#name", "User Two");
    await page2.fill("#email", email);
    await page2.fill("#password", "password456");
    await page2.click('button[type="submit"]');

    await expect(page2.locator("body")).toContainText("Email already exists");
    await ctx2.close();
  });
});

test.describe("Login", () => {
  test("logs in with valid credentials and reaches chat", async ({ browser }) => {
    const email = uniqueEmail();

    const signupCtx = await browser.newContext();
    const signupPage = await signupCtx.newPage();
    await signupPage.goto("/users/new");
    await signupPage.fill("#name", "Login Test");
    await signupPage.fill("#email", email);
    await signupPage.fill("#password", "password123");
    await signupPage.click('button[type="submit"]');
    await expect(signupPage).toHaveURL(/\/users\/\d+/);
    await signupCtx.close();

    const loginCtx = await browser.newContext();
    const loginPage = await loginCtx.newPage();
    await loginPage.goto("/login");
    await loginPage.fill("#email", email);
    await loginPage.fill("#password", "password123");
    await loginPage.click('button[type="submit"]');

    await expect(loginPage).toHaveURL("/chat");
    await expect(loginPage.locator("body")).toContainText("Chat rooms");
    await loginCtx.close();
  });

  test("rejects invalid credentials", async ({ page }) => {
    await page.goto("/login");
    await page.fill("#email", uniqueEmail());
    await page.fill("#password", "wrongpassword");
    await page.click('button[type="submit"]');

    await expect(page.locator("body")).toContainText("Email or password is incorrect");
  });

  test("redirects unauthenticated user from chat to login", async ({ page }) => {
    await page.goto("/chat");
    await expect(page).toHaveURL(/\/login/);
  });
});
