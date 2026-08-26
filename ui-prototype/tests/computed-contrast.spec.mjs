import { expect, test } from '@playwright/test'

const luminance = ([red, green, blue]) =>
  [red, green, blue]
    .map((channel) => {
      const value = channel / 255
      return value <= 0.04045
        ? value / 12.92
        : ((value + 0.055) / 1.055) ** 2.4
    })
    .reduce(
      (sum, channel, index) =>
        sum + channel * [0.2126, 0.7152, 0.0722][index],
      0,
    )

const contrast = (foreground, background) => {
  const values = [luminance(foreground), luminance(background)].sort(
    (left, right) => right - left,
  )
  return (values[0] + 0.05) / (values[1] + 0.05)
}

async function computedPairs(locator) {
  return locator.evaluateAll((nodes) => {
    const parse = (value) => {
      const channels = value.match(/[\d.]+/g)?.map(Number) ?? []
      return {
        rgb: channels.slice(0, 3),
        alpha: channels[3] ?? 1,
      }
    }

    const backgroundFor = (node) => {
      let current = node
      while (current) {
        const background = parse(getComputedStyle(current).backgroundColor)
        if (background.rgb.length === 3 && background.alpha > 0.99) {
          return background.rgb
        }
        current = current.parentElement
      }
      return [255, 255, 255]
    }

    return nodes.map((node) => ({
      text: node.textContent?.trim() ?? '',
      foreground: parse(getComputedStyle(node).color).rgb,
      background: backgroundFor(node),
      className: String(node.className),
    }))
  })
}

async function expectTextContrast(locator, label) {
  const pairs = await computedPairs(locator)
  expect(pairs.length, `${label} must resolve at least one visible text node`).toBeGreaterThan(
    0,
  )
  for (const pair of pairs) {
    expect(
      contrast(pair.foreground, pair.background),
      `${label} "${pair.text}" (${pair.className})`,
    ).toBeGreaterThanOrEqual(4.5)
  }
}

test('rendered badge axes meet normal-text contrast', async ({ page }) => {
  await page.goto('/')
  await expectTextContrast(
    page.locator('.status-badge__axis:visible'),
    'landing status axis',
  )

  await page.goto('/?screen=workspace')
  await expectTextContrast(
    page.locator('.status-badge__axis:visible'),
    'workspace status axis',
  )
})

test('code line numbers meet contrast in default, selected, and error rows', async ({
  page,
}) => {
  await page.goto('/?screen=workspace')
  await expectTextContrast(
    page.locator('.code-pane li > span:visible'),
    'ready code line number',
  )

  await page.goto('/?screen=workspace&state=error')
  await expectTextContrast(
    page.locator('.code-pane li > span:visible'),
    'error code line number',
  )
})
