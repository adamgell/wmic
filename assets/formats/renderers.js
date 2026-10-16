// Native MSXML presentation helpers. Operates only on the supplied CIM document.
function documentOf(set) { var n=set.item(0); return n.nodeType==9?n:n.ownerDocument; }
function select(n,path) { return n.selectNodes(path); }
function attr(n,name) { var a=n.attributes.getNamedItem(name); return a?String(a.nodeValue):""; }
function text(n) {
    if(!n)return "";
    if(n.nodeType==2||n.nodeType==3||n.nodeType==4)return String(n.nodeValue);
    var out="",children=n.childNodes;for(var i=0;i<children.length;i++)out+=text(children.item(i));return out;
}
function properties(n) { return select(n,"PROPERTY|PROPERTY.ARRAY|PROPERTY.REFERENCE"); }
function html(s) { return String(s).replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;"); }
function namespacePath(n) {
    var out="",host=n.selectSingleNode("HOST"),parts=select(n,"LOCALNAMESPACEPATH/NAMESPACE");
    if(host)out="\\\\"+text(host);
    for(var i=0;i<parts.length;i++)out+="\\"+attr(parts.item(i),"NAME");
    return out+":";
}
function reference(p,quoted,trailing) {
    var out="",paths=select(p,"VALUE.REFERENCE//NAMESPACEPATH"),names=select(p,"VALUE.REFERENCE//INSTANCENAME");
    for(var i=0;i<paths.length;i++)out+=namespacePath(paths.item(i));
    for(var i=0;i<names.length;i++) {
        var n=names.item(i),keys=select(n,"KEYBINDING");out+=attr(n,"CLASSNAME");
        for(var k=0;k<keys.length;k++) {
            var key=keys.item(k);out+=(k==0?".":"")+attr(key,"NAME")+'="'+text(key.selectSingleNode("KEYVALUE"))+'"';
            if(trailing!==null)out+=trailing;else if(k+1<keys.length)out+=",";
        }
    }
    return quoted?'"'+out+'"':out;
}
function value(p,mode,norefcomma) {
    if(p.nodeName=="PROPERTY.REFERENCE")return reference(p,mode!="table",mode=="csv"?(norefcomma=="true"?" ":","):null);
    if(p.nodeName=="PROPERTY.ARRAY") {
        var array=p.selectSingleNode("VALUE.ARRAY");if(!array)return "";
        var items=select(array,"VALUE"),out="{";
        for(var i=0;i<items.length;i++) {
            if(i)out+=mode=="csv"?";":mode=="list"?",":", ";
            var q=mode=="csv"?"":attr(p,"TYPE")=="string"?'"':attr(p,"TYPE")=="char16"?"'":"";
            out+=q+text(items.item(i))+q;
        }
        return out+"}";
    }
    var out=text(p);
    return mode=="table"?out.replace(/[\r\n]/g,""):out;
}
function width(s) {var n=0;for(var i=0;i<s.length;i++)n+=s.charCodeAt(i)>255?2:1;return n;}
function padding(n) {return new Array(n+1).join(" ");}
function table(root) {
    var rows=select(documentOf(root),"//INSTANCE"),data=[],heads=[],sizes=[],columns=0;
    if(rows.length>2049)throw new Error("WMIC table row limit");
    for(var r=0;r<rows.length;r++) {
        var props=properties(rows.item(r)),row=[];columns=props.length;
        if(columns>129)throw new Error("WMIC table column limit");
        for(var c=0;c<props.length;c++) {var p=props.item(c);if(r==0)heads[c]=attr(p,"NAME");row[c]=value(p,"table","");}
        data.push(row);
    }
    for(var c=0;c<columns;c++) {
        sizes[c]=width(heads[c]||"");
        for(var r=0;r<data.length;r++)sizes[c]=Math.max(sizes[c],width(data[r][c]||""));
    }
    var out="";
    for(var c=0;c<columns;c++){var h=heads[c]||"";out+=h+padding(sizes[c]-width(h)+2);}out+="\r\n";
    for(var r=0;r<data.length;r++){for(var c=0;c<columns;c++){var s=data[r][c]||"";out+=s+padding(sizes[c]-width(s)+2);}out+="\r\n";}
    return out;
}
function csv(root,norefcomma) {
    var doc=documentOf(root),headers=select(doc,"/COMMAND/RESULTS[1]/CIM/INSTANCE[1]//PROPERTY|/COMMAND/RESULTS[1]/CIM/INSTANCE[1]//PROPERTY.ARRAY|/COMMAND/RESULTS[1]/CIM/INSTANCE[1]//PROPERTY.REFERENCE"),out="\r\nNode,";
    for(var c=0;c<headers.length;c++){if(c)out+=",";out+=html(attr(headers.item(c),"NAME"));}
    var groups=select(doc,"/COMMAND/RESULTS");
    for(var g=0;g<groups.length;g++) {
        var rows=select(groups.item(g),"CIM/INSTANCE");
        for(var r=0;r<rows.length;r++) {
            out+="\r\n"+html(attr(groups.item(g),"NODE"))+",";
            var props=properties(rows.item(r));for(var c=0;c<props.length;c++){if(c)out+=",";out+=html(value(props.item(c),"csv",norefcomma));}
        }
    }
    return out;
}
function list(root) {
    var rows=select(documentOf(root),"//INSTANCE"),out="\r\n";
    for(var r=0;r<rows.length;r++) {
        out+="\r\n";var props=properties(rows.item(r));
        for(var c=0;c<props.length;c++){var p=props.item(c);out+=html(attr(p,"NAME"))+"="+html(value(p,"list",""))+"\r\n";}
        out+="\r\n";
    }
    return out+"\r\n";
}
function span(kind,s) {return '<SPAN CLASS="'+kind+'">'+s+'</SPAN>';}
function normalized(s) {return String(s).replace(/[ \t\r\n]+/g," ").replace(/^ | $/g,"");}
function mofValue(type,s) {
    if(type=="string"||type=="datetime")return span("mofstring",'"'+html(normalized(s))+'"');
    if(type=="char16")return span("mofchar","'"+html(normalized(s))+"'");
    return span("mofvalue",html(s));
}
function mofProperty(p) {
    var out='<DD>\r\n<DIV CLASS="mofproperty">\r\n<SPAN CLASS="mofqualifierset"> </SPAN>\r\n',type=attr(p,"TYPE");
    out+=span("mofkeyword",html(type))+'\r\n'+span("mofproperty",html(attr(p,"NAME")))+'\r\n';
    var array=p.nodeName=="PROPERTY.ARRAY",values=select(p,"VALUE.ARRAY/VALUE"),scalar=p.selectSingleNode("VALUE");
    if(array)out+=span("mofsymbol","[")+span("mofsymbol","]")+'\r\n';
    if(array&&p.selectSingleNode("VALUE.ARRAY")) {
        out+=span("mofsymbol","=")+span("mofsymbol","{");
        for(var i=0;i<values.length;i++){if(i)out+=",";out+=mofValue(type,text(values.item(i)));}
        out+=span("mofsymbol","}")+'\r\n';
    } else if(scalar)out+=span("mofsymbol","=")+mofValue(type,text(scalar));
    return out+span("mofsymbol",";")+'<BR />\r\n</DIV>\r\n</DD>\r\n';
}
function mof(root) {
    var rows=select(documentOf(root),"/COMMAND//RESULTS//CIM//INSTANCE"),out="";
    for(var r=0;r<rows.length;r++) {
        var n=rows.item(r);
        out+='<DIV CLASS="mofinstance">\r\n<SPAN CLASS="mofqualifierset"> <br /></SPAN>\r\n'+span("mofkeyword","instance of")+' '+html(attr(n,"CLASSNAME"))+' '+span("mofsymbol","{")+'<BR /> <DL> ';
        var scalar=select(n,"PROPERTY"),arrays=select(n,"PROPERTY.ARRAY"),props=[];
        for(var c=0;c<scalar.length;c++)props.push(scalar.item(c));for(var c=0;c<arrays.length;c++)props.push(arrays.item(c));
        for(var c=0;c<props.length;c++)out+=mofProperty(props[c]);
        out+='</DL>\r\n'+span("mofsymbol","};")+'\r\n</DIV>\r\n';
    }
    return out;
}
var dot='<span style="height:1px;overflow-y:hidden">.</span>';
var heading='<tr style="background-color:#a0a0ff;font:10pt Tahoma;font-weight:bold;" align="left">';
function rowStyle(position) {return '<tr style="background-color:#'+(position%2?'f0f0f0':'e0f0f0')+';font:10pt Tahoma;">\r\n';}
function htmlValue(p) {
    if(p.nodeName=="PROPERTY.REFERENCE")return html(reference(p,false,null));
    if(p.nodeName=="PROPERTY.ARRAY")return "\r\n\t"+html(value(p,"list",""))+"\r\n";
    return html(value(p,"list",""));
}
function htable(root,sortby,datatype) {
    var doc=documentOf(root),all=select(doc,"/COMMAND/RESULTS/CIM/INSTANCE"),groups=select(doc,"/COMMAND/RESULTS"),out='<html>\r\n<H3>'+all.length+' Instances of '+(all.length?html(attr(all.item(0),"CLASSNAME")):"")+'</H3>\r\n<table border="1">\r\n'+heading+'\r\n<td>Node</td>\r\n';
    var heads=all.length?properties(all.item(0)):null;
    if(heads)for(var c=0;c<heads.length;c++)out+='<td>'+html(attr(heads.item(c),"NAME"))+'</td>\r\n';
    out+='</tr>\r\n';
    for(var g=0;g<groups.length;g++) {
        var group=groups.item(g),nodes=select(group,"CIM/INSTANCE"),rows=[];
        var groupStyle=rowStyle(g+2);out+=nodes.length?groupStyle:groupStyle.slice(0,-2);
        for(var r=0;r<nodes.length;r++)rows.push(nodes.item(r));
        if(sortby)rows.sort(function(a,b){var pa=properties(a),pb=properties(b),va="",vb="";for(var i=0;i<pa.length;i++)if(attr(pa.item(i),"NAME")==sortby)va=text(pa.item(i));for(var i=0;i<pb.length;i++)if(attr(pb.item(i),"NAME")==sortby)vb=text(pb.item(i));if(datatype=="number")return Number(va)-Number(vb);return va<vb?-1:va>vb?1:0;});
        for(var r=0;r<rows.length;r++) {
            out+=rowStyle(r+1)+'<td align="left">'+html(attr(group,"NODE"))+'</td>\r\n';var props=properties(rows[r]);
            for(var c=0;c<props.length;c++)out+='<td>'+htmlValue(props.item(c))+dot+'</td>\r\n';out+='</tr>\r\n';
        }
        out+='</tr>\r\n';
    }
    return out+'</table>\r\n</html>\r\n';
}
function hform(root) {
    var groups=select(documentOf(root),"/COMMAND/RESULTS"),out='<html>\r\n';
    for(var g=0;g<groups.length;g++) {
        var group=groups.item(g),rows=select(group,"CIM/INSTANCE");
        out+='<H3>Node: '+html(attr(group,"NODE"))+' - '+rows.length+' Instances of '+(rows.length?html(attr(rows.item(0),"CLASSNAME")):"")+'</H3>\r\n<table border="1">'+(rows.length?'\r\n':'');
        for(var r=0;r<rows.length;r++) {
            out+=heading+'<td colspan="2">INSTANCE'+dot+'</td></tr>\r\n<tr style="background-color:#c0c0c0;font:8pt Tahoma;">\r\n<td>Property Name</td><td>Value</td>\r\n</tr>\r\n';
            var props=properties(rows.item(r));for(var c=0;c<props.length;c++){var p=props.item(c);out+=rowStyle(c+1)+'<td>'+html(attr(p,"NAME"))+'</td>\r\n<td>'+htmlValue(p)+dot+'</td>\r\n</tr>\r\n';}
            out+='<tr style="background-color:#ffffff;font:10pt Tahoma;font-weight:bold;"><td colspan="2">'+dot+'</td></tr>\r\n';
        }
        out+='</table>\r\n';
    }
    return out+'</html>\r\n';
}
function xmlAttributes(n) {
    var attributes=select(n,"@*"),out="";
    for(var a=0;a<attributes.length;a++) {
        var x=attributes.item(a);if(a)out+="\r\n";
        out+='<SPAN STYLE="color:navy">'+html(x.baseName||x.nodeName)+'="<SPAN STYLE="color:black">'+html(text(x))+'</SPAN>"</SPAN>';
    }
    return out;
}
function xmlNode(n) {
    if(n.nodeType==3||n.nodeType==4)return html(text(n));
    if(n.nodeType==7)return '<DIV STYLE="margin-left:1em; color:maroon">&lt;?'+html(n.baseName||n.nodeName)+xmlAttributes(n)+'?&gt;</DIV>\r\n';
    if(n.nodeType!=1)return "";
    var name=html(n.baseName||n.nodeName),attributes=xmlAttributes(n);
    if(!n.hasChildNodes())return '<DIV STYLE="margin-left:1em; color:gray">\r\n        &lt;'+name+'&gt;'+attributes+'/&gt;\r\n     </DIV>\r\n';
    var out='<DIV STYLE="margin-left:1em">\r\n<SPAN STYLE="color:gray">&lt;'+name+attributes+'&gt;</SPAN>';
    var children=n.childNodes;for(var c=0;c<children.length;c++)out+=xmlNode(children.item(c));
    return out+'<SPAN STYLE="color:gray">&lt;/'+name+'&gt;</SPAN>\r\n</DIV>\r\n';
}
function xml(root) {
    var doc=documentOf(root),out='<DIV STYLE="font-family:Courier; font-size:10pt; margin-bottom:2em">\r\n',children=doc.childNodes;
    for(var i=0;i<children.length;i++)out+=xmlNode(children.item(i));
    return out+'</DIV>\r\n';
}
