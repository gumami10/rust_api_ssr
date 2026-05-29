import { test, expect, type Page, type BrowserContext, type Browser } from "@playwright/test";

const uniqueEmail = () => `test-${Date.now()}-${Math.random().toString(36).slice(2)}@example.com`;

interface TestUser {
  context: BrowserContext;
  page: Page;
  name: string;
}

async function signUpAndLogin(browser: Browser, name: string): Promise<TestUser> {
  const context = await browser.newContext();
  const page = await context.newPage();
  const email = uniqueEmail();
  const password = "password123";

  await page.goto("/users/new");
  await page.fill("#name", name);
  await page.fill("#email", email);
  await page.fill("#password", password);
  await page.click('button[type="submit"]');
  await expect(page).toHaveURL(/\/users\/\d+/);

  await page.goto("/chat");
  await expect(page).toHaveURL("/chat");

  return { context, page, name };
}

async function getUserIdFromProfile(page: Page): Promise<string> {
  const url = page.url();
  const match = url.match(/\/users\/(\d+)/);
  if (match) return match[1];

  await page.goto("/users");
  const bodyText = await page.locator("body").textContent();
  const idMatch = bodyText?.match(/\/users\/(\d+)/);
  return idMatch ? idMatch[1] : "";
}

test.describe("E2E encrypted chat", () => {
  test("two users can exchange encrypted messages in a 1-to-1 room", async ({ browser }) => {
    const userA = await signUpAndLogin(browser, "Alice E2E");
    const userB = await signUpAndLogin(browser, "Bob E2E");

    const userBProfile = await userB.context.newPage();
    await userBProfile.goto("/users");
    const links = await userBProfile.locator('a[href*="/users/"]').all();
    let userBId = "";
    for (const link of links) {
      const href = await link.getAttribute("href");
      const text = await link.textContent();
      if (href && text && text.includes("Bob E2E")) {
        const m = href.match(/\/users\/(\d+)/);
        if (m) { userBId = m[1]; break; }
      }
    }
    await userBProfile.close();

    expect(userBId).toBeTruthy();

    await userA.page.goto("/chat");
    await expect(userA.page.locator("#chat-messages")).toBeVisible();

    const participantCheckbox = userA.page.locator(`input[data-participant-checkbox][value="${userBId}"]`);
    await expect(participantCheckbox).toBeVisible({ timeout: 5_000 });
    await participantCheckbox.check();

    const roomName = `E2E Test Room ${Date.now()}`;
    await userA.page.fill("#room-name", roomName);
    await userA.page.click('form[action="/chat/rooms"] button[type="submit"]');

    await expect(userA.page).toHaveURL(/\/chat\/rooms\/\d+/, { timeout: 10_000 });

    await userB.page.goto("/chat");
    const roomLink = userB.page.locator(`a[href*="/chat/rooms/"]`).filter({ hasText: roomName });
    await expect(roomLink).toBeVisible({ timeout: 10_000 });
    await roomLink.click();

    await expect(userB.page).toHaveURL(/\/chat\/rooms\/\d+/, { timeout: 10_000 });

    await expect(userA.page.locator("#e2e-badge")).toBeVisible({ timeout: 5_000 });
    await expect(userB.page.locator("#e2e-badge")).toBeVisible({ timeout: 5_000 });

    await userA.page.waitForTimeout(4000);
    await userB.page.waitForTimeout(4000);

    const secretMessage = `Secret message ${Date.now()}`;
    await userA.page.fill("#chat-body", secretMessage);
    await userA.page.click('#chat-form button[type="submit"]');

    await expect(userA.page.locator("#chat-messages")).toContainText(secretMessage, { timeout: 15_000 });
    await expect(userB.page.locator("#chat-messages")).toContainText(secretMessage, { timeout: 15_000 });

    const replyMessage = `Secret reply ${Date.now()}`;
    await userB.page.fill("#chat-body", replyMessage);
    await userB.page.click('#chat-form button[type="submit"]');

    await expect(userB.page.locator("#chat-messages")).toContainText(replyMessage, { timeout: 15_000 });
    await expect(userA.page.locator("#chat-messages")).toContainText(replyMessage, { timeout: 15_000 });

    await userA.context.close();
    await userB.context.close();
  });

  test("shows E2E badge for 1-to-1 rooms", async ({ browser }) => {
    const userA = await signUpAndLogin(browser, "Badge Alice");
    const userB = await signUpAndLogin(browser, "Badge Bob");

    const userBProfile = await userB.context.newPage();
    await userBProfile.goto("/users");
    const links = await userBProfile.locator('a[href*="/users/"]').all();
    let userBId = "";
    for (const link of links) {
      const href = await link.getAttribute("href");
      const text = await link.textContent();
      if (href && text && text.includes("Badge Bob")) {
        const m = href.match(/\/users\/(\d+)/);
        if (m) { userBId = m[1]; break; }
      }
    }
    await userBProfile.close();

    expect(userBId).toBeTruthy();

    await userA.page.goto("/chat");

    const participantCheckbox = userA.page.locator(`input[data-participant-checkbox][value="${userBId}"]`);
    await expect(participantCheckbox).toBeVisible({ timeout: 5_000 });
    await participantCheckbox.check();

    await userA.page.fill("#room-name", `Badge Room ${Date.now()}`);
    await userA.page.click('form[action="/chat/rooms"] button[type="submit"]');

    await expect(userA.page).toHaveURL(/\/chat\/rooms\/\d+/, { timeout: 10_000 });
    await expect(userA.page.locator("#e2e-badge")).toBeVisible({ timeout: 5_000 });

    await userA.context.close();
    await userB.context.close();
  });
});
