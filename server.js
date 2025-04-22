const express = require('express');
const bodyParser = require('body-parser');
const cors = require('cors');

const app = express();
app.use(cors());
app.use(bodyParser.json());
app.use(express.static('public'));

// 存储用户和文件传输信息
const users = new Map(); // key: userId, value: { res (EventSource response) }
const fileTransfers = new Map(); // key: transferId, value: file transfer info
const pendingTransfers = new Map(); // key: receiverId, value: transferId
const router = express.Router();
app.use(router);

// 生成随机ID
function generateId() {
    return Math.floor(Math.random() * 1000000);
}

// 用户连接端点
router.get('/connect', (req, res) => {
    const userId = generateId();
    users.set(userId, { res: null });
    
    res.setHeader('Content-Type', 'application/json');
    res.send(JSON.stringify({ userId }));
});

// 发送方初始化文件传输
router.post('/init-transfer', (req, res) => {
    const { senderId, receiverId, fileName, fileSize } = req.body;
    
    if (!users.has(receiverId)) {
        return res.status(404).json({ error: 'Receiver not found' });
    }
    
    const transferId = generateId();
    const transferInfo = {
        transferId,
        senderId,
        receiverId,
        fileName,
        fileSize,
        chunks: [],
        completed: false
    };
    
    fileTransfers.set(transferId, transferInfo);
    pendingTransfers.set(receiverId, transferId);
    
    // 通知接收方有文件等待接收
    const receiver = users.get(receiverId);
    if (receiver && receiver.res) {
        receiver.res.write(`event: transfer-waiting\ndata: ${JSON.stringify({
            transferId,
            senderId,
            fileName,
            fileSize
        })}\n\n`);
    }
    
    res.json({ transferId });
});

// 接收方轮询等待传输
router.get('/wait-for-transfer/:receiverId', (req, res) => {
    const receiverId = parseInt(req.params.receiverId);
    
    if (!users.has(receiverId)) {
        return res.status(404).send('User not found');
    }
    
    // 设置 EventSource 头
    res.setHeader('Content-Type', 'text/event-stream');
    res.setHeader('Cache-Control', 'no-cache');
    res.setHeader('Connection', 'keep-alive');
    
    // 存储响应对象以便后续发送事件
    users.set(receiverId, { res });
    
    // 检查是否有等待中的传输
    const transferId = pendingTransfers.get(receiverId);
    if (transferId) {
        const transfer = fileTransfers.get(transferId);
        res.write(`event: transfer-waiting\ndata: ${JSON.stringify({
            transferId: transfer.transferId,
            senderId: transfer.senderId,
            fileName: transfer.fileName,
            fileSize: transfer.fileSize
        })}\n\n`);
    }
    
    // 客户端断开连接时清理
    req.on('close', () => {
        users.delete(receiverId);
    });
});

// 接收方接受传输
router.post('/accept-transfer', (req, res) => {
    const { transferId, receiverId } = req.body;
    const transfer = fileTransfers.get(transferId);
    
    if (!transfer || transfer.receiverId !== receiverId) {
        return res.status(404).json({ error: 'Transfer not found' });
    }
    
    // 通知发送方可以开始传输
    const sender = users.get(transfer.senderId);
    if (sender && sender.res) {
        sender.res.write(`event: transfer-accepted\ndata: ${JSON.stringify({
            transferId
        })}\n\n`);
    }
    
    res.json({ success: true });
});

// 发送方上传文件分片
router.post('/upload-chunk/:transferId', (req, res) => {
    const transferId = parseInt(req.params.transferId);
    const { chunk, index, isLast } = req.body;
    const transfer = fileTransfers.get(transferId);
    
    if (!transfer) {
        return res.status(404).json({ error: 'Transfer not found' });
    }
    
    // 存储分片
    transfer.chunks[index] = chunk;
    
    // 转发分片给接收方
    const receiver = users.get(transfer.receiverId);
    if (receiver && receiver.res) {
        receiver.res.write(`event: chunk-received\ndata: ${JSON.stringify({
            index,
            chunk,
            isLast
        })}\n\n`);
    }
    
    if (isLast) {
        transfer.completed = true;
        fileTransfers.delete(transferId);
    }
    
    res.json({ success: true });
});

const PORT = 9843;
app.listen(PORT, '0.0.0.0', () => {
    console.log(`Server running on http://localhost:${PORT}`);
});