// Explicitly opt-in smoke test: creates a task in the seeded dev workspace and moves it to trash.
const {chromium,expect}=require('@playwright/test');
(async()=>{
const browser=await chromium.launch({executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH});
try {
const a=await browser.newContext(), b=await browser.newContext();
const p=await a.newPage(), q=await b.newPage();
const base=process.env.ORBIT_SMOKE_URL;
if(!base) throw new Error('Set ORBIT_SMOKE_URL to an existing development server');
for(const page of [p,q]) {

await page.goto(base+'/login');
await page.getByLabel('Email',{exact:true}).fill('test@example.com');
await page.getByLabel('Password',{exact:true}).fill('password');
await page.getByRole('button',{name:'Sign in',exact:true}).click();
await page.waitForURL('**/tasks');
await expect(page.getByText('Connecting to live updates…')).toHaveCount(0,{timeout:15000});
}
await p.getByRole('button',{name:'New task',exact:true}).click();
await p.waitForURL(/\/tasks\/[^?]+/);
const title='Live smoke '+Date.now();
await p.getByRole('textbox',{name:'Task title',exact:true}).fill(title);
await p.getByRole('textbox',{name:'Task title',exact:true}).press('Enter');
await expect(q.getByText(title,{exact:true})).toBeVisible({timeout:15000});
console.log('PASS second session sees newly created task');
await q.goto(p.url());
await expect(q.getByRole('textbox',{name:'Task title',exact:true})).toHaveValue(title);
await q.getByRole('textbox',{name:'Task title',exact:true}).fill('unsaved local draft');
await p.getByRole('textbox',{name:'Task title',exact:true}).fill(title+' updated');
await p.getByRole('textbox',{name:'Task title',exact:true}).press('Enter');
await q.waitForTimeout(1500);
await expect(q.getByRole('textbox',{name:'Task title',exact:true})).toHaveValue('unsaved local draft');
await q.getByRole('textbox',{name:'Task title',exact:true}).fill(title);
await q.getByRole('textbox',{name:'Task title',exact:true}).blur();
await expect(q.getByRole('textbox',{name:'Task title',exact:true})).toHaveValue(title+' updated',{timeout:15000});
console.log('PASS remote updates preserve focused drafts');
console.log('PASS second session sees remote title update');
const comment='Live comment '+Date.now();
await p.locator('.tasks-native-composer textarea').fill(comment);
await p.getByRole('button',{name:'Send',exact:true}).click();
await expect(q.getByText(comment,{exact:true})).toBeVisible({timeout:15000});
console.log('PASS second session sees new comment');
await b.setOffline(true);
await p.getByRole('textbox',{name:'Task title',exact:true}).fill(title+' reconnect');
await p.getByRole('textbox',{name:'Task title',exact:true}).press('Enter');
await b.setOffline(false);
await expect(q.getByRole('textbox',{name:'Task title',exact:true})).toHaveValue(title+' reconnect',{timeout:20000});
console.log('PASS reconnect catches up');
p.once('dialog',d=>d.accept());
await p.getByRole('button',{name:'Delete',exact:true}).click();
await p.waitForURL(/\/tasks(?:\?|$)/);
console.log('PASS smoke task moved to trash');
} finally {await browser.close()}
})().catch(e=>{console.error(e);process.exit(1)});
