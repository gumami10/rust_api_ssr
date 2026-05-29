import { test, expect, type Page, type BrowserContext } from "@playwright/test";

const uniqueEmail = () => `test-${Date.now()}-${Math.random().toString(36).slice(2)}@example.com`;

async function signUpAndLogin(context: BrowserContext, name: string): Promise<Page> {
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

  return page;
}

test.describe("General chat", () => {
  test("sends and displays a message", async ({ context }) => {
    const page = await signUpAndLogin(context, "Chat User");

    await expect(page.locator("#chat-messages")).toBeVisible();

    const messageText = `Hello from E2E test ${Date.now()}`;
    await page.fill("#chat-body", messageText);
    await page.click('#chat-form button[type="submit"]');

    await expect(page.locator("#chat-messages")).toContainText(messageText, { timeout: 10_000 });
  });

  test("displays the general room badge", async ({ context }) => {
    const page = await signUpAndLogin(context, "Badge User");
    await expect(page.locator("body")).toContainText("General");
  });

  test("shows signed-in user name", async ({ context }) => {
    const name = "Visible User";
    const page = await signUpAndLogin(context, name);
    await expect(page.locator("body")).toContainText(`Signed in as ${name}`);
  });

  test("broadcasts messages to another user in real time", async ({ browser }) => {
    const ctxA = await browser.newContext();
    const ctxB = await browser.newContext();
    const pageA = await signUpAndLogin(ctxA, "User A");
    const pageB = await signUpAndLogin(ctxB, "User B");

    const messageText = `Broadcast test ${Date.now()}`;
    await pageA.fill("#chat-body", messageText);
    await pageA.click('#chat-form button[type="submit"]');

    await expect(pageA.locator("#chat-messages")).toContainText(messageText, { timeout: 10_000 });
    await expect(pageB.locator("#chat-messages")).toContainText(messageText, { timeout: 15_000 });

    await ctxA.close();
    await ctxB.close();
  });
});
