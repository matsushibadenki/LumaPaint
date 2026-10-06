import type {ColorMode, ColorProfile} from './bridge';
export const modeLabels = {en:{rgb:'RGB',cmyk:'CMYK',grayscale:'Grayscale',lab:'Lab'},ja:{rgb:'RGB',cmyk:'CMYK',grayscale:'グレースケール',lab:'Lab'},'zh-CN':{rgb:'RGB',cmyk:'CMYK',grayscale:'灰度',lab:'Lab'}};
export const colorModes: ColorMode[] = ['rgb','cmyk','grayscale','lab'];
export const colorProfiles: {value:ColorProfile;label:string;mode:ColorMode}[] = [
{value:'srgb',label:'sRGB IEC61966-2.1',mode:'rgb'}, {value:'displayP3',label:'Display P3',mode:'rgb'}, {value:'adobeRgb1998',label:'Adobe RGB (1998)',mode:'rgb'}, {value:'japanColor2001Coated',label:'Japan Color 2001 Coated',mode:'cmyk'}, {value:'grayD65',label:'Gray D65 (sRGB luminance)',mode:'grayscale'}, {value:'labD50',label:'CIE Lab D50',mode:'lab'}];
export function defaultProfile(mode:ColorMode):ColorProfile {return colorProfiles.find(p=>p.mode===mode)!.value;}
export function compatibleProfile(mode:unknown,profile:unknown) {return colorProfiles.some(p=>p.mode===mode&&p.value===profile);}
