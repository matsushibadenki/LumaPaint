import { memo, useCallback, useEffect, useId, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Locale } from '../i18n';

type Provider = 'openai' | 'gemini';
import { emptyConditions, emptyPrompt, promptFieldLabels, promptFields, promptOptions, promptSections, type AiPrompt, type PromptField } from '../ai-prompt-options';
type Prompt = AiPrompt;
const labels = {
  ja:{getKey:'APIキーを取得（ブラウザーで開く）',provider:'サービス',model:'モデル',name:'ブックマーク名',camera:'カメラタイプ',film:'フィルムタイプ',lens:'レンズタイプ',lighting:'照明',aperture:'絞り',shutter:'シャッタースピード',iso:'感度（ISO）',bokeh:'ぼけ',style:'生成タイプ',text:'プロンプト',edit:'画像を修正',generate:'生成',generating:'生成中…',cancel:'適用をキャンセル',save:'保存 / 更新',remove:'削除',bookmark:'保存したプロンプト',new:'新しいプロンプト',done:'画像を適用しました',hint:'APIキーは環境設定の「AI生成」で設定します。修正を有効にして画像レイヤーの範囲を選択すると、その範囲だけを修正します。選択範囲が無い場合は新しいレイヤーに生成します。',transfer:'生成時にプロンプト、修正時に選択範囲の画像を選択したサービスに送信します。APIの利用料金が発生します。',key:'APIキー',stored:'キー保存済み',missing:'未設定',verify:'保存・認証',verified:'認証成功',deleteKey:'キーを削除',keyHint:'キーはmacOSのキーチェーンに保存します。認証はキーの接続確認で、画像モデルの利用権限は生成時に確認されます。',styles:['映画風','テレビ風','写真','イラスト風','漫画風','アニメ風']},
  en:{getKey:'Get an API key (opens in browser)',provider:'Provider',model:'Model',name:'Bookmark name',camera:'Camera type',film:'Film type',lens:'Lens type',lighting:'Lighting',aperture:'Aperture',shutter:'Shutter speed',iso:'ISO',bokeh:'Bokeh',style:'Generation style',text:'Prompt',edit:'Edit image',generate:'Generate',generating:'Generating…',cancel:'Cancel application',save:'Save / Update',remove:'Delete',bookmark:'Saved prompts',new:'New prompt',done:'Image applied',hint:'Configure API keys in Preferences → AI Generation. Enable editing and select a region on an image layer to edit only that region. Without a selection, a new layer is created.',transfer:'Generation sends your prompt to the selected provider; editing also sends the selected image region. API usage is billed by the provider.',key:'API key',stored:'Key saved',missing:'Not configured',verify:'Save & Authenticate',verified:'Authentication succeeded',deleteKey:'Delete key',keyHint:'Keys are stored in the macOS Keychain. Authentication checks the key connection; image model access is checked when generating.',styles:['Cinematic','Television','Photography','Illustration','Manga','Anime']},
  'zh-CN':{getKey:'获取API密钥（在浏览器中打开）',provider:'服务',model:'模型',name:'书签名称',camera:'相机类型',film:'胶片类型',lens:'镜头类型',lighting:'照明',aperture:'光圈',shutter:'快门速度',iso:'感光度（ISO）',bokeh:'散景',style:'生成类型',text:'提示词',edit:'修改图像',generate:'生成',generating:'正在生成…',cancel:'取消应用',save:'保存 / 更新',remove:'删除',bookmark:'已保存的提示词',new:'新提示词',done:'已应用图像',hint:'在偏好设置的“AI生成”中配置API密钥。启用修改并选择图像图层中的区域后，仅修改该区域。没有选区时将在新图层中生成。',transfer:'生成时将提示词发送到所选服务；修改时还会发送选区图像。服务商将收取API使用费用。',key:'API密钥',stored:'密钥已保存',missing:'未配置',verify:'保存并验证',verified:'验证成功',deleteKey:'删除密钥',keyHint:'密钥存储在macOS钥匙串中。验证检查密钥连接，生成时检查图像模型的访问权限。',styles:['电影风','电视风','摄影','插画风','漫画风','动画风']},
};
const panelLabels = {
  ja: { details:'詳細設定', guidance:'イメージの主役、動き、場所、仕上がりを自由に記述してください。', placeholder:'例：雨上がりの路地を歩く人物。濡れた石畳に街灯が映り込み、静かな物語を感じる一枚。', none:'指定しない', custom:'自由入力…', customValue:'自由入力', reset:'設定をクリア', help:'使い方と送信について', conditions:'設定済み', bookmarkSaved:'プロンプトを保存しました', bookmarkDeleted:'ブックマークを削除しました', filmNote:'撮影条件やフィルムは、表現を指示するための指定です。', saveName:'次回も使うプロンプトに名前を付ける' },
  en: { details:'Detailed settings', guidance:'Describe your subject, action, setting and intended finish.', placeholder:'Example: A person walking through an alley after rain. Streetlights reflect on wet cobblestones, suggesting a quiet story.', none:'Unspecified', custom:'Custom…', customValue:'Custom value', reset:'Clear settings', help:'Usage & data sharing', conditions:'configured', bookmarkSaved:'Prompt saved', bookmarkDeleted:'Bookmark deleted', filmNote:'Camera and film settings describe the intended visual treatment.', saveName:'Name this prompt to reuse it later' },
  'zh-CN': { details:'详细设置', guidance:'自由描述主体、动作、场景和期望效果。', placeholder:'例如：雨后小巷中行走的人，湿润石板路映着街灯，画面充满安静的故事感。', none:'不指定', custom:'自定义…', customValue:'自定义内容', reset:'清除设置', help:'使用说明与数据发送', conditions:'已设置', bookmarkSaved:'提示词已保存', bookmarkDeleted:'书签已删除', filmNote:'拍摄和胶片设置用于描述预期的视觉表现。', saveName:'为提示词命名，以便下次使用' },
};

const PresetField = memo(function PresetField({field,value,locale,disabled,onChange}:{field:PromptField;value:string;locale:Locale;disabled:boolean;onChange:(field:PromptField,value:string)=>void}) {
  const [custom,setCustom]=useState(false);
  const groups=promptOptions[field];
  const known=groups.some(group=>group.choices.some(choice=>choice.value===value));
  const isCustom=custom || (!!value && !known);
  const label=promptFieldLabels[field][locale];
  const t=panelLabels[locale];
  return <div className="ai-preset-field" data-field={field}>
    <label><span>{label}</span><select disabled={disabled} value={isCustom?'__custom__':value} onChange={event=>{
      if(event.target.value==='__custom__')setCustom(true);
      else {setCustom(false);onChange(field,event.target.value);}
    }}>
      <option value="">{t.none}</option>
      {groups.map(group=><optgroup key={group.label.en} label={group.label[locale]}>{group.choices.map(choice=><option key={choice.value} value={choice.value}>{choice.label[locale]}</option>)}</optgroup>)}
      <option value="__custom__">{t.custom}</option>
    </select></label>
    {isCustom && <input className="ai-custom-value" aria-label={`${label} · ${t.customValue}`} disabled={disabled} value={value} placeholder={t.customValue} maxLength={512} onChange={event=>onChange(field,event.target.value)}/>}
  </div>;
});
const models:Record<Provider,string>={openai:'gpt-image-2.5-sunburst',gemini:'gemini-nano-banana-2.1'};
const modelOptions:Record<Provider,string[]>={openai:[models.openai],gemini:[models.gemini,'gemini-3.1-flash-lite-image','gemini-3-pro-image']};
export function AiGenerationPanel({locale}:{locale:Locale}) {
  const t={...labels[locale],...panelLabels[locale]};
  const modelListId=useId();
  const [provider,setProvider]=useState<Provider>('openai');
  const [model,setModel]=useState(models.openai);
  const [prompt,setPrompt]=useState<Prompt>({...emptyPrompt});
  const [library,setLibrary]=useState<Prompt[]>([]);
  const [selected,setSelected]=useState('');
  const [edit,setEdit]=useState(false);
  const [busy,setBusy]=useState(false);
  const [saving,setSaving]=useState(false);
  const running=useRef(false);
  const [status,setStatus]=useState('');
  const [error,setError]=useState('');
  const [fieldVersion,setFieldVersion]=useState(0);
  const disabled=busy||saving;
  const configured=promptFields.filter(field=>prompt[field].trim()).length;
  const updateField=useCallback((field:PromptField,value:string)=>setPrompt(previous=>({...previous,[field]:value})),[]);

  useEffect(()=>{
    let live=true;
    const load=()=>invoke<Prompt[]>('ai_bookmarks',{action:'list'}).then(value=>{if(live)setLibrary(value);}).catch(reason=>{if(live)setError(String(reason));});
    void load();
    const stop=listen('ai-bookmarks-changed',()=>void load()).catch(()=>()=>{});
    return()=>{live=false;void stop.then(fn=>fn());};
  },[]);
  async function bookmark(action:'save'|'delete') {
    if(running.current)return;
    running.current=true;setSaving(true);setError('');setStatus('');
    try {
      const list=await invoke<Prompt[]>('ai_bookmarks',{action,prompt,name:selected});
      setLibrary(list);setSelected(action==='save'?prompt.name:'');
      setStatus(action==='save'?t.bookmarkSaved:t.bookmarkDeleted);
    }catch(reason){setError(String(reason));}
    finally{running.current=false;setSaving(false);}
  }
  async function generate() {
    if(running.current)return;
    running.current=true;setBusy(true);setError('');setStatus('');
    try {await invoke('ai_generate',{request:{provider,model,prompt,edit}});setStatus(t.done);}
    catch(reason){setError(String(reason));}
    finally{running.current=false;setBusy(false);}
  }
  return <div className="ai-generation" onKeyDown={event=>event.stopPropagation()} aria-busy={busy}>
    <div className="ai-generation-toolbar">
      <label>{t.provider}<select disabled={disabled} value={provider} onChange={event=>{
        const value=event.target.value as Provider;setProvider(value);setModel(models[value]);
      }}><option value="openai">OpenAI</option><option value="gemini">Google Gemini</option></select></label>
      <label className="ai-model">{t.model}<input list={modelListId} disabled={disabled} value={model} onChange={event=>setModel(event.target.value)} maxLength={100}/><datalist id={modelListId}>{modelOptions[provider].map(id=><option key={id} value={id}/>)}</datalist></label>
      <label className="ai-bookmark-picker">{t.bookmark}<select disabled={disabled} value={selected} onChange={event=>{
        setSelected(event.target.value);
        setPrompt({...emptyPrompt,...library.find(item=>item.name===event.target.value)});
        setFieldVersion(previous=>previous+1);setStatus('');setError('');
      }}><option value="">{t.new}</option>{library.map(item=><option key={item.name}>{item.name}</option>)}</select></label>
    </div>
    <div className="ai-generation-body">
      <div className="ai-prompt-column">
        <label className="ai-prompt"><span>{t.text}</span><textarea disabled={disabled} value={prompt.text} placeholder={t.placeholder} onChange={event=>setPrompt(previous=>({...previous,text:event.target.value}))} maxLength={16000} rows={8}/></label>
        <p className="ai-prompt-guidance">{t.guidance}</p>
        <details className="ai-help"><summary>{t.help}</summary><p>{t.hint}</p><p>{t.transfer}</p><p>{t.filmNote}</p></details>
      </div>
      <div className="ai-details-column">
        <header className="ai-details-heading"><h3>{t.details}</h3><span>{configured} / {promptFields.length} {t.conditions}</span><button type="button" disabled={disabled||!configured} onClick={()=>{setPrompt(previous=>({...previous,...emptyConditions}));setFieldVersion(previous=>previous+1);}}>{t.reset}</button></header>
        {promptSections.map((section,index)=><details key={section.title.en} className="ai-option-section" open={index===0?true:undefined}>
          <summary><span>{section.title[locale]}</span><small>{section.fields.filter(field=>prompt[field].trim()).length} / {section.fields.length}</small></summary>
          <div className={`ai-conditions ${index===3?'ai-exposure-fields':''}`}>
            {section.fields.map(field=><PresetField key={`${field}-${fieldVersion}`} field={field} value={prompt[field]} locale={locale} disabled={disabled} onChange={updateField}/>)}
          </div>
        </details>)}
      </div>
    </div>
    <footer className="ai-generation-footer">
      <div className="ai-bookmark-actions"><label className="ai-bookmark-name"><span>{t.name}</span><input disabled={disabled} value={prompt.name} placeholder={t.saveName} maxLength={200} onChange={event=>setPrompt(previous=>({...previous,name:event.target.value}))}/></label><button type="button" disabled={disabled||!prompt.name.trim()||!prompt.text.trim()} onClick={()=>void bookmark('save')}>{t.save}</button><button type="button" disabled={disabled||!selected} onClick={()=>void bookmark('delete')}>{t.remove}</button></div>
      <div className="ai-generate-actions"><label className="ai-edit"><input type="checkbox" disabled={disabled} checked={edit} onChange={event=>setEdit(event.target.checked)}/>{t.edit}</label>{busy&&<button type="button" onClick={()=>void invoke('ai_cancel').catch(reason=>setError(String(reason)))}>{t.cancel}</button>}<button type="button" className="ai-generate-button" disabled={disabled||!prompt.text.trim()} onClick={()=>void generate()}>{busy?t.generating:t.generate}</button></div>
      {(status||error)&&<p role={error?'alert':'status'} className={`ai-feedback ${error?'ai-error':''}`}>{error||status}</p>}
    </footer>
  </div>;
}
const apiKeyPages:Record<Provider,string>={openai:'https://platform.openai.com/api-keys',gemini:'https://aistudio.google.com/api-keys'};
export function AiSettings({locale}:{locale:Locale}) {
  const t=labels[locale]; const [keys,setKeys]=useState({openai:'',gemini:''});const [status,setStatus]=useState<Record<Provider,string>>({openai:'',gemini:''});const [busy,setBusy]=useState<Provider|null>(null);
  useEffect(()=>{let live=true;for(const provider of ['openai','gemini'] as const)invoke<boolean>('ai_credentials',{provider,action:'status'}).then(v=>{if(live)setStatus(s=>({...s,[provider]:v?t.stored:t.missing}));}).catch(e=>{if(live)setStatus(s=>({...s,[provider]:String(e)}));});return()=>{live=false;};},[t]);
  async function credentials(provider:Provider,action:'verify'|'delete') {setBusy(provider);try{if(action==='verify'&&keys[provider].trim()){await invoke('ai_credentials',{provider,action:'save',key:keys[provider]});setKeys(s=>({...s,[provider]:''}));}const ok=await invoke<boolean>('ai_credentials',{provider,action});if(action==='delete')setKeys(s=>({...s,[provider]:''}));setStatus(s=>({...s,[provider]:action==='delete'?t.missing:ok?t.verified:t.missing}));}catch(e){setStatus(s=>({...s,[provider]:String(e)}));}finally{setBusy(null);}}
  return <div className="ai-settings"><h3>AI</h3><p>{t.keyHint}</p>{(['openai','gemini'] as const).map(provider=><fieldset key={provider}><legend>{provider==='openai'?'OpenAI':'Google Gemini'}</legend><a className="ai-api-key-link" href={apiKeyPages[provider]} target="_blank" rel="noopener noreferrer" onClick={event=>{if(isTauri()){event.preventDefault();void invoke('ai_open_key_page',{provider}).catch(error=>setStatus(s=>({...s,[provider]:String(error)})));}}}>{t.getKey}<span aria-hidden="true"> ↗</span></a><label>{t.key}<input type="password" autoComplete="off" spellCheck={false} value={keys[provider]} disabled={busy!==null} onChange={e=>setKeys({...keys,[provider]:e.target.value})}/></label><div><button disabled={busy!==null} onClick={()=>void credentials(provider,'verify')}>{t.verify}</button><button disabled={busy!==null} onClick={()=>void credentials(provider,'delete')}>{t.deleteKey}</button></div><p role="status">{status[provider]}</p></fieldset>)}</div>;
}
