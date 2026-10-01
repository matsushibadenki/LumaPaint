import { CheckMenuItem, IconMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu, type MenuOptions } from '@tauri-apps/api/menu';

type Item = NonNullable<MenuOptions['items']>[number];
type Resource = { close(): Promise<void> };

// Tauri 2.12 removes a menu item's action channel when its Rust wrapper is dropped.
// Create every item as a retained resource instead of temporary inline options.
export async function createNativeMenu(options: MenuOptions) {
  const resources: Resource[] = [];
  const close = async () => {
    for (const resource of resources.splice(0).reverse()) await resource.close();
  };
  const createItem = async (item: Item): Promise<Item> => {
    if ('rid' in item) return item;
    const resource = 'items' in item
      ? await Submenu.new({ ...item, items: await Promise.all((item.items ?? []).map(createItem)) })
      : 'item' in item ? await PredefinedMenuItem.new(item)
      : 'checked' in item ? await CheckMenuItem.new(item)
      : 'icon' in item ? await IconMenuItem.new(item)
      : await MenuItem.new(item);
    resources.push(resource);
    return resource;
  };
  try {
    const items = await Promise.all((options.items ?? []).map(createItem));
    const menu = await Menu.new({ ...options, items });
    resources.push(menu);
    return { popup: menu.popup.bind(menu), close };
  } catch (error) {
    await close();
    throw error;
  }
}

export type NativeMenu = Awaited<ReturnType<typeof createNativeMenu>>;
