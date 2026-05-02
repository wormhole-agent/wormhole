// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
/**
 * screenshot_and_send.js
 * Takes a screenshot of the active browser tab and sends it to the configured default chat via Telegram.
 * Usage: node screenshot_and_send.js [optional tab URL filter]
 */
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');
const https = require('https');
const FormData = require('form-data');
const { getByLabel } = require('./secrets/accessor');

const BOT_TOKEN = process.env.TELEGRAM_BOT_TOKEN || getByLabel('Bot Token', 'Telegram Bot');
const DEFAULT_CHAT_ID = process.env.TELEGRAM_DEFAULT_CHAT_ID || getByLabel('Default Chat ID', 'Telegram Bot');

async function sendPhoto(filePath, caption) {
  const form = new FormData();
  form.append('chat_id', DEFAULT_CHAT_ID);
  form.append('photo', fs.createReadStream(filePath));
  if (caption) form.append('caption', caption);

  return new Promise((resolve, reject) => {
    const req = https.request({
      hostname: 'api.telegram.org',
      path: `/bot${BOT_TOKEN}/sendPhoto`,
      method: 'POST',
      headers: form.getHeaders(),
    }, res => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => resolve(JSON.parse(data)));
    });
    req.on('error', reject);
    form.pipe(req);
  });
}

(async () => {
  const urlFilter = process.argv[2] || null;
  
  const browser = await chromium.connectOverCDP('http://localhost:9222');
  const ctx = browser.contexts()[0];
  const pages = await ctx.pages();
  
  let target;
  if (urlFilter) {
    target = pages.find(p => p.url().includes(urlFilter));
  }
  if (!target) {
    // Use the most recently focused tab (highest index)
    target = pages[pages.length - 1];
  }
  
  await target.bringToFront();
  await target.waitForTimeout(500);
  
  const screenshotPath = path.join(process.env.TEMP, `larry_screenshot_${Date.now()}.png`);
  await target.screenshot({ path: screenshotPath });
  
  const url = target.url();
  const caption = `📸 Screenshot from Home\n${url.substring(0, 80)}`;
  
  const result = await sendPhoto(screenshotPath, caption);
  console.log('Sent:', result.ok, 'message_id:', result.result?.message_id);
  
  fs.unlinkSync(screenshotPath);
  await browser.close();
})();
