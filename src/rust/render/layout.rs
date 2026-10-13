use super::{Renderer, TextStyle, FontFamily, Cow};
use std::cell::RefCell;
use std::ffi::c_void;
use std::slice;
use taffy::prelude::*;
use taffy::style::*;
use taffy::tree::{LayoutInput, LayoutOutput};

#[repr(C)]
pub struct Property { name: *const u8, name_len: usize, value: *const u8, value_len: usize }
#[repr(C)]
pub struct Input {
    parent: i32, display: u32, properties: *const Property, properties_len: usize,
    text: *const u8, text_len: usize, font_size: f32, intrinsic_width: f32, intrinsic_height: f32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Output {
    x: f32, y: f32, width: f32, height: f32, content_width: f32, content_height: f32,
    border_left: f32, border_top: f32, border_right: f32, border_bottom: f32,
    padding_left: f32, padding_top: f32, padding_right: f32, padding_bottom: f32,
    fixed: u32,
    grid_columns: *const u8, grid_columns_len: usize, grid_rows: *const u8, grid_rows_len: usize,
}
impl Default for Output { fn default()->Self { unsafe {std::mem::zeroed()} } }
struct Strings(Vec<Box<[u8]>>);
fn retain(strings:&mut Strings,value:String)->(*const u8,usize) {
    let data=value.into_bytes().into_boxed_slice();let result=(data.as_ptr(),data.len());strings.0.push(data);result
}
#[no_mangle]
pub unsafe extern "C" fn lp_dom_layout_free(state:*mut c_void) { if !state.is_null(){drop(Box::from_raw(state as *mut Strings));} }
struct Text { text: String, font_size: f32, family: String, width: f32, height: f32 }
thread_local! { static FONTS: RefCell<Renderer> = RefCell::new(Renderer::new(&[])); }

unsafe fn string<'a>(ptr: *const u8, len: usize) -> &'a str {
    if len == 0 { return ""; }
    std::str::from_utf8(slice::from_raw_parts(ptr, len)).unwrap_or("")
}
fn unit(value: &str, viewport: Size<f32>, font: f32) -> String {
    for (suffix, scale) in [("rem",16.0),("em",font),("vw",viewport.width/100.0),
        ("vh",viewport.height/100.0),("vmin",viewport.width.min(viewport.height)/100.0),
        ("vmax",viewport.width.max(viewport.height)/100.0),("pt",4.0/3.0)] {
        if let Some(number)=value.strip_suffix(suffix).and_then(|s|s.trim().parse::<f32>().ok()) {
            return format!("{}px",number*scale);
        }
    }
    if value == "0" { "0px".into() } else { value.into() }
}
fn apply(style: &mut Style, name: &str, raw: &str, viewport: Size<f32>, font: f32) {
    let value=unit(raw.trim(),viewport,font);
    macro_rules! parsed { ($field:expr) => { if let Ok(v)=value.parse() { $field=v; } }; }
    match name {
        "display" => style.display=match raw {"none"=>Display::None,"flex"|"inline-flex"=>Display::Flex,
            "grid"|"inline-grid"=>Display::Grid,"flow-root"=>Display::FlowRoot,_=>Display::Block},
        "position" => style.position=match raw {"absolute"=>Position::Absolute,"fixed"=>Position::Fixed,
            "relative"=>Position::Relative,"sticky"=>Position::Sticky,_=>Position::Static},
        "box-sizing" => style.box_sizing=if raw=="border-box"{BoxSizing::BorderBox}else{BoxSizing::ContentBox},
        "width" => parsed!(style.size.width), "height" => parsed!(style.size.height),
        "min-width" => parsed!(style.min_size.width), "min-height" => parsed!(style.min_size.height),
        "max-width" => parsed!(style.max_size.width), "max-height" => parsed!(style.max_size.height),
        "top" => parsed!(style.inset.top), "right" => parsed!(style.inset.right),
        "bottom" => parsed!(style.inset.bottom), "left" => parsed!(style.inset.left),
        "flex-grow" => parsed!(style.flex_grow), "flex-shrink" => parsed!(style.flex_shrink),
        "flex-basis" => parsed!(style.flex_basis), "flex-direction" => parsed!(style.flex_direction),
        "flex-wrap" => parsed!(style.flex_wrap), "align-items" => parsed!(style.align_items),
        "align-content" => parsed!(style.align_content), "justify-content" => parsed!(style.justify_content),
        "justify-items" => parsed!(style.justify_items),
        "align-self" => { if let Ok(v)=value.parse(){style.align_self=Some(v)} },
        "justify-self" => { if let Ok(v)=value.parse(){style.justify_self=Some(v)} },
        "row-gap" => parsed!(style.gap.height), "column-gap" => parsed!(style.gap.width),
        "gap" => { let mut parts=raw.split_whitespace();if let Some(a)=parts.next(){
            apply(style,"row-gap",a,viewport,font);apply(style,"column-gap",parts.next().unwrap_or(a),viewport,font)} },
        "flex" => {
            let parts: Vec<_>=raw.split_whitespace().collect();
            if raw=="none" {style.flex_grow=0.0;style.flex_shrink=0.0;style.flex_basis=Dimension::auto();}
            else if raw=="auto" {style.flex_grow=1.0;style.flex_shrink=1.0;style.flex_basis=Dimension::auto();}
            else if let Some(grow)=parts.first().and_then(|v|v.parse::<f32>().ok()) {
                style.flex_grow=grow;style.flex_shrink=parts.get(1).and_then(|v|v.parse().ok()).unwrap_or(1.0);
                style.flex_basis=parts.get(2).and_then(|v|v.parse().ok()).unwrap_or(Dimension::percent(0.0));
            }
        },
        "grid-template-columns"|"grid-template-rows" => {
            if let Ok(tracks)=raw.parse::<GridTemplateTracks<String,GridTemplateComponent<String>>>() {
                if name.ends_with("columns"){style.grid_template_columns=tracks.tracks;}else{style.grid_template_rows=tracks.tracks;}
            }
        },
        "grid-auto-flow" => parsed!(style.grid_auto_flow),
        "grid-column-start" => parsed!(style.grid_column.start), "grid-column-end" => parsed!(style.grid_column.end),
        "grid-row-start" => parsed!(style.grid_row.start), "grid-row-end" => parsed!(style.grid_row.end),
        "overflow"|"overflow-x"|"overflow-y" => {
            let v=match raw {"hidden"=>Overflow::Hidden,"clip"=>Overflow::Clip,"auto"|"scroll"=>Overflow::Scroll,_=>Overflow::Visible};
            if name!="overflow-y"{style.overflow.x=v;}if name!="overflow-x"{style.overflow.y=v;}
        },
        "aspect-ratio" => {
            let mut parts=raw.split('/');let a=parts.next().and_then(|s|s.trim().parse::<f32>().ok());
            let b=parts.next().and_then(|s|s.trim().parse::<f32>().ok()).unwrap_or(1.0);
            style.aspect_ratio=a.filter(|v|*v>0.0&&b>0.0).map(|v|v/b);
        },
        _ if name=="margin"||name=="padding"||name=="border-width" => {
            let p:Vec<_>=raw.split_whitespace().collect();if !p.is_empty()&&p.len()<=4 {
                let values=[p[0],*p.get(1).unwrap_or(&p[0]),*p.get(2).unwrap_or(&p[0]),*p.get(3).unwrap_or(p.get(1).unwrap_or(&p[0]))];
                let base=if name=="border-width"{"border"}else{name};
                for (edge,v) in ["top","right","bottom","left"].iter().zip(values){
                    let key=if base=="border"{format!("border-{}-width",edge)}else{format!("{}-{}",base,edge)};
                    apply(style,&key,v,viewport,font);
                }
            }
        },
        "margin-top"=>parsed!(style.margin.top),"margin-right"=>parsed!(style.margin.right),
        "margin-bottom"=>parsed!(style.margin.bottom),"margin-left"=>parsed!(style.margin.left),
        "padding-top"=>parsed!(style.padding.top),"padding-right"=>parsed!(style.padding.right),
        "padding-bottom"=>parsed!(style.padding.bottom),"padding-left"=>parsed!(style.padding.left),
        "border-top-width"=>parsed!(style.border.top),"border-right-width"=>parsed!(style.border.right),
        "border-bottom-width"=>parsed!(style.border.bottom),"border-left-width"=>parsed!(style.border.left),
        "border" => {if let Some(v)=raw.split_whitespace().next(){apply(style,"border-width",v,viewport,font)}},
        _ => {},
    }
}
fn measure(input: LayoutInput, style: &Style, context: Option<&mut Text>) -> LayoutOutput {
    taffy::compute_leaf_layout(input,style,|_,_|0.0,|known,available|{
        let Some(context)=context else {return Size::ZERO;};
        if context.text.is_empty(){return Size{width:known.width.unwrap_or(context.width),height:known.height.unwrap_or(context.height)}}
        FONTS.with(|fonts|{
            let mut renderer=fonts.borrow_mut();
            let root=TextStyle{font_size:context.font_size,font_family:FontFamily::Source(Cow::Borrowed(&context.family)),..Default::default()};
            let Renderer{fcx,lcx,..}=&mut *renderer;
            let mut builder=lcx.tree_builder(fcx,1.0,true,&root);builder.push_text(&context.text);
            let(mut text_layout,_)=builder.build();
            let width=known.width.or(match available.width{AvailableSpace::Definite(v)=>Some(v),AvailableSpace::MinContent=>Some(0.0),_=>None});
            text_layout.break_all_lines(width);
            Size{width:known.width.unwrap_or(text_layout.width()),height:known.height.unwrap_or(text_layout.height())}
        })
    })
}

#[no_mangle]
pub unsafe extern "C" fn lp_dom_layout(inputs:*const Input,count:usize,width:f32,height:f32,outputs:*mut Output,state:*mut *mut c_void)->i32 {
    std::panic::catch_unwind(||{
        if count==0{return Ok(());}
        let inputs=slice::from_raw_parts(inputs,count);let outputs=slice::from_raw_parts_mut(outputs,count);
        let mut strings=Strings(Vec::new());
        let viewport=Size{width,height};let mut tree=TaffyTree::<Text>::new();let mut ids=Vec::with_capacity(count);
        let mut children=vec![Vec::new();count];
        for input in inputs.iter(){
            let mut style=Style{display:match input.display{1=>Display::Flex,2=>Display::Grid,3=>Display::None,_=>Display::Block},box_sizing:BoxSizing::ContentBox,..Default::default()};
            let mut family="sans-serif".to_string();
            if input.properties_len>0{for p in slice::from_raw_parts(input.properties,input.properties_len){
                let name=string(p.name,p.name_len);let value=string(p.value,p.value_len);
                if name=="font-family"{family=value.to_owned();}else{apply(&mut style,name,value,viewport,input.font_size)}
            }}
            let text=string(input.text,input.text_len).to_owned();
            let id=tree.new_leaf_with_context(style,Text{text,font_size:input.font_size,family,width:input.intrinsic_width,height:input.intrinsic_height}).map_err(|_|())?;
            ids.push(id);if input.parent>=0{children[input.parent as usize].push(id);}
        }
        for (id,children) in ids.iter().zip(children){tree.set_children(*id,&children).map_err(|_|())?;}
        let viewport_root=tree.new_with_children(Style{display:Display::Block,position:Position::Relative,size:Size{width:Dimension::length(width),height:Dimension::length(height)},..Default::default()},&[ids[0]]).map_err(|_|())?;
        tree.compute_layout_with_measure(viewport_root,Size{width:AvailableSpace::Definite(width),height:AvailableSpace::Definite(height)},|input,_,context,style|measure(input,style,context)).map_err(|_|())?;
        for (index,id) in ids.iter().enumerate(){
            let mut columns=(std::ptr::null(),0);let mut rows=(std::ptr::null(),0);
            if let taffy::tree::DetailedLayoutInfo::Grid(info)=tree.detailed_layout_info(*id) {
                columns=retain(&mut strings,info.columns.positions.iter().map(|p|format!("{}px",p.end-p.start)).collect::<Vec<_>>().join(" "));
                rows=retain(&mut strings,info.rows.positions.iter().map(|p|format!("{}px",p.end-p.start)).collect::<Vec<_>>().join(" "));
            }
            let l=tree.layout(*id).map_err(|_|())?;let parent=inputs[index].parent;
            let mut x=l.location.x;let mut y=l.location.y;
            let fixed=tree.style(*id).map_err(|_|())?.position==Position::Fixed;
            if parent>=0&&!fixed{x+=outputs[parent as usize].x;y+=outputs[parent as usize].y;}
            outputs[index]=Output{x,y,width:l.size.width,height:l.size.height,
                content_width:l.scrollable_overflow_rect.right,content_height:l.scrollable_overflow_rect.bottom,
                border_left:l.border.left,border_top:l.border.top,border_right:l.border.right,border_bottom:l.border.bottom,
                padding_left:l.padding.left,padding_top:l.padding.top,padding_right:l.padding.right,padding_bottom:l.padding.bottom,
                fixed:fixed as u32,grid_columns:columns.0,grid_columns_len:columns.1,grid_rows:rows.0,grid_rows_len:rows.1};
        }
        *state=Box::into_raw(Box::new(strings)) as *mut c_void;
        Ok::<(),()>(())
    }).map_or(-2,|result|if result.is_ok(){0}else{-1})
}
