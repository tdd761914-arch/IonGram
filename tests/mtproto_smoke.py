"""Independent wire test: Telethon performs RSA/DH, MTProto encryption and TCP framing.
The small schema adapter lets an older installed Telethon exercise API layers 229/231.
Run against a fresh local IonGram: python tests/mtproto_smoke.py DATA_DIR PORT
"""
import asyncio, collections, gzip, io, json, logging, pathlib, re, sqlite3, struct, sys
from telethon.network import MTProtoSender
from telethon.network.connection import ConnectionTcpFull, ConnectionTcpAbridged, ConnectionTcpIntermediate, ConnectionTcpObfuscated
from telethon.crypto import rsa
from telethon.tl.tlobject import TLRequest
ROOT=pathlib.Path(__file__).resolve().parents[1]
class Codec:
    def __init__(self,layer):
        src=(ROOT/'schema'/('api_229.tl' if layer==229 else 'api.tl')).read_text()
        self.ids={}; self.names={}
        for line in src.splitlines():
            m=re.match(r'(\w[\w.]*)#([0-9a-f]+)(.*?) = ',line)
            if not m or m[1]=='vector': continue
            fields=[]
            for item in m[3].split():
                if ':' not in item or item.startswith('{'): continue
                k,t=item.split(':',1); fields.append((k,t.lstrip('!')))
            cid=int(m[2],16);self.ids[cid]=(m[1],fields);self.names[m[1]]=(cid,fields)
    def enc(self,v,t='Object'):
        if t in ('int','#'):return struct.pack('<I',int(v)&0xffffffff)
        if t=='long':return struct.pack('<q',int(v))
        if t=='double':return struct.pack('<d',v)
        if t=='true':return b''
        if t in ('string','bytes'):
            data=v.encode() if isinstance(v,str) else bytes(v)
            h=bytes([len(data)]) if len(data)<254 else b'\xfe'+struct.pack('<I',len(data))[:3]
            return h+data+b'\0'*(-len(h+data)%4)
        if t.startswith('Vector<'):return struct.pack('<Ii',0x1cb5c415,len(v))+b''.join(self.enc(x,t[7:-1]) for x in v)
        if isinstance(v,bool):return struct.pack('<I',0x997275b5 if v else 0xbc799737)
        cid,fields=self.names[v['_']];flags=collections.defaultdict(int)
        for k,t in fields:
            if '?' in t and k in v and v[k] is not None and (t.split('?')[1]!='true' or v[k]):
                flag,bit=t.split('?')[0].split('.');flags[flag]|=1<<int(bit)
        b=struct.pack('<I',cid)
        for k,t in fields:
            if t=='#':b+=self.enc(flags[k],'#');continue
            if '?' in t:
                f,t=t.split('?');flag,bit=f.split('.')
                if not flags[flag]&(1<<int(bit)):continue
            b+=self.enc(v[k],t)
        return b
    def dec(self,r,t='Object'):
        if t in ('int','#'):return struct.unpack('<i',r.read(4))[0]
        if t=='long':return struct.unpack('<q',r.read(8))[0]
        if t=='double':return struct.unpack('<d',r.read(8))[0]
        if t=='true':return True
        if t in ('string','bytes'):
            n=r.read(1)[0];h=1
            if n==254:n=int.from_bytes(r.read(3),'little');h=4
            b=r.read(n);r.read(-(n+h)%4);return b.decode() if t=='string' else b
        if t.startswith('Vector<'):
            assert struct.unpack('<I',r.read(4))[0]==0x1cb5c415
            return [self.dec(r,t[7:-1]) for _ in range(self.dec(r,'int'))]
        cid=struct.unpack('<I',r.read(4))[0]
        if cid in (0x997275b5,0xbc799737):return cid==0x997275b5
        if cid==0x1cb5c415:return [self.dec(r) for _ in range(self.dec(r,'int'))]
        name,fields=self.ids[cid];v={'_':name}
        for k,t in fields:
            if '?' in t:
                f,t=t.split('?');flag,bit=f.split('.')
                if not v[flag]&(1<<int(bit)):continue
            v[k]=self.dec(r,t)
        return v
class Request(TLRequest):
    CONSTRUCTOR_ID=0;SUBCLASS_OF_ID=0
    def __init__(self,c,v):self.c=c;self.v=v
    def _bytes(self):return self.c.enc(self.v)
    def read_result(self,r):return self.c.dec(r)
    def to_dict(self):return self.v
async def main():
    data=pathlib.Path(sys.argv[1]);port=int(sys.argv[2]);rsa.add_key((data/'public.pem').read_text(),old=False)
    loggers=collections.defaultdict(lambda:logging.getLogger('wire'))
    for index,(layer,transport) in enumerate([(229,ConnectionTcpFull),(231,ConnectionTcpAbridged),(229,ConnectionTcpIntermediate),(231,ConnectionTcpObfuscated)]):
        c=Codec(layer);sender=MTProtoSender(None,loggers=loggers,retries=0,auto_reconnect=False,updates_queue=asyncio.Queue())
        await sender.connect(transport('127.0.0.1',port,2,loggers=loggers))
        async def call(name,**args):return await asyncio.wait_for(sender.send(Request(c,{'_':name,**args})),15)
        config=await call('invokeWithLayer',layer=layer,query={'_':'help.getConfig'})
        assert config['this_dc']==2
        phone=f'1555000{index:04}'
        sent=await call('auth.sendCode',phone_number=phone,api_id=123,api_hash='local',settings={'_':'codeSettings'})
        with sqlite3.connect(data/'iongram.sqlite') as db:state=json.loads(db.execute('select data from state').fetchone()[0])
        code=state['pending'][phone]['code']
        auth=await call('auth.signIn',phone_number=phone,phone_code_hash=sent['phone_code_hash'],phone_code=code)
        uid=auth['user']['id'];me={'_':'inputPeerSelf'}
        await call('account.updateProfile',first_name=f'Wire{index}')
        await call('account.updateUsername',username=f'wireuser{index}')
        users=await call('users.getUsers',id=[{'_':'inputUserSelf'}]);assert users[0]['id']==uid
        result=await call('messages.sendMessage',peer=me,message='hello Rust',random_id=100+index)
        mid=next(u['id'] for u in result['updates'] if u['_']=='updateMessageID')
        history=await call('messages.getHistory',peer=me,offset_id=0,offset_date=0,add_offset=0,limit=10,max_id=0,min_id=0,hash=0)
        assert history['messages'][0]['message']=='hello Rust'
        await call('messages.editMessage',peer=me,id=mid,message='edited')
        group=await call('messages.createChat',users=[],title='Wire group')
        assert group['updates']['chats'][0]['title']=='Wire group'
        ch=await call('channels.createChannel',broadcast=True,title='Wire channel',about='test')
        ch=ch['chats'][0];channel={'_':'inputChannel','channel_id':ch['id'],'access_hash':ch['access_hash']}
        await call('channels.updateUsername',channel=channel,username=f'wirechannel{index}')
        full=await call('channels.getFullChannel',channel=channel);assert full['full_chat']['participants_count']==1
        cp={'_':'inputPeerChannel',**{k:channel[k] for k in ('channel_id','access_hash')}}
        await call('messages.sendMessage',peer=cp,message='post',random_id=200+index)
        inv=await call('messages.exportChatInvite',peer=cp)
        assert (await call('messages.checkChatInvite',hash=inv['link'].split('+')[1]))['_']=='chatInviteAlready'
        tgs=gzip.compress(json.dumps({'v':'5.7','w':512,'h':512,'fr':60,'ip':0,'op':120,'layers':[]}).encode())
        fid=9000+index
        assert await call('upload.saveFilePart',file_id=fid,file_part=0,bytes=tgs)
        media=await call('messages.uploadMedia',peer=me,media={'_':'inputMediaUploadedDocument','file':{'_':'inputFile','id':fid,'parts':1,'name':'test.tgs','md5_checksum':''},'mime_type':'application/x-tgsticker','attributes':[{'_':'documentAttributeAnimated'}]})
        doc=media['document']
        got=await call('upload.getFile',location={'_':'inputDocumentFileLocation','id':doc['id'],'access_hash':doc['access_hash'],'file_reference':doc['file_reference'],'thumb_size':''},offset=0,limit=1024)
        assert got['bytes']==tgs
        stars=await call('payments.getStarsStatus',peer=me);assert stars['balance']['amount']==0
        state=await call('updates.getState');assert state['pts']>=2
        diff=await call('updates.getDifference',pts=0,date=0,qts=0);assert diff['_']=='updates.difference'
        await call('messages.deleteMessages',id=[mid],revoke=True)
        await sender.disconnect()
        print(f'PASS layer {layer} {transport.__name__}: DH/login/users/DM/group/channel/invite/TGS/files/Stars/Updates',flush=True)
asyncio.run(main())
