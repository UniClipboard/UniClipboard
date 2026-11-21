import {defineConfig} from 'vite';
import tailwindcss from '@tailwindcss/vite';
import path from 'node:path';
export default defineConfig({plugins:[tailwindcss()],resolve:{alias:{'@':path.resolve('upstream')}},esbuild:{jsx:'automatic'},server:{fs:{allow:[path.resolve('.'),path.resolve('../../../app/desktop/boardui/assets')]}}});
