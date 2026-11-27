/** 烹饪字符逐词匹配，长词优先，提示文本和食材始终进行 HTML 转义。 */
export const COOKING_METHODS = [
  {
    "name": "炒",
    "pinyin": "chǎo",
    "description": "猛油快翻食材致熟，如小炒肉"
  },
  {
    "name": "煎",
    "pinyin": "jiān",
    "description": "少油慢火煎至两面金黄，如煎鸡蛋"
  },
  {
    "name": "炸",
    "pinyin": "zhá",
    "description": "食材入多量热油致脆，如炸薯条"
  },
  {
    "name": "蒸",
    "pinyin": "zhēng",
    "description": "借水蒸气热力致熟，如清蒸鱼"
  },
  {
    "name": "炖",
    "pinyin": "dùn",
    "description": "文火慢煮使食材软烂，如炖排骨"
  },
  {
    "name": "煮",
    "pinyin": "zhǔ",
    "description": "食材置沸水中煮熟，如煮面条"
  },
  {
    "name": "汆",
    "pinyin": "cuān",
    "description": "食材入沸水快速烫熟，如汆丸子"
  },
  {
    "name": "焗",
    "pinyin": "jú",
    "description": "密封环境用热力焖熟，如芝士焗饭"
  },
  {
    "name": "烤",
    "pinyin": "kǎo",
    "description": "明火或烤箱干热烤熟，如烤羊肉"
  },
  {
    "name": "煨",
    "pinyin": "wēi",
    "description": "瓦坛密封文火慢炖入味，如煨猪蹄"
  },
  {
    "name": "焖",
    "pinyin": "mèn",
    "description": "盖紧锅盖文火慢煮，如油焖大虾"
  },
  {
    "name": "烩",
    "pinyin": "huì",
    "description": "多食材加汤水同煮融合，如烩三鲜"
  },
  {
    "name": "熬",
    "pinyin": "āo",
    "description": "慢火久煮浓缩汤汁，如熬鸡汤"
  },
  {
    "name": "煸",
    "pinyin": "biān",
    "description": "小火慢炒逼出油脂香味，如煸干辣椒"
  },
  {
    "name": "爆",
    "pinyin": "bào",
    "description": "热油快炒加酱汁速熟，如爆炒腰花"
  },
  {
    "name": "烘",
    "pinyin": "hōng",
    "description": "烤箱低温烘烤，如烘面包"
  },
  {
    "name": "焙",
    "pinyin": "bèi",
    "description": "小火烘干食材，如焙茶叶"
  },
  {
    "name": "灼",
    "pinyin": "zhuó",
    "description": "薄片食材沸水速熟蘸酱，如白灼虾"
  },
  {
    "name": "焯",
    "pinyin": "chāo",
    "description": "食材入沸水初步熟处理，如焯青菜"
  },
  {
    "name": "炟",
    "pinyin": "dá",
    "description": "蔬菜加枧水沸水慢煮保绿，如炟芥蓝"
  },
  {
    "name": "烚",
    "pinyin": "xiá",
    "description": "大量沸水炊软韧质食材，如烚大闸蟹"
  },
  {
    "name": "滚",
    "pinyin": "gǔn",
    "description": "沸水涌动煮出食材异味，如滚汤"
  },
  {
    "name": "炆",
    "pinyin": "wén",
    "description": "南方式文火慢煮，如炆牛腩"
  },
  {
    "name": "煀",
    "pinyin": "qū",
    "description": "加大量香料盖焖致熟，如煀狗肉"
  },
  {
    "name": "焐",
    "pinyin": "wù",
    "description": "荷叶包裹泥封入炭火烤熟"
  },
  {
    "name": "熯",
    "pinyin": "hàn",
    "description": "炒干食材水分辟除异味"
  },
  {
    "name": "煏",
    "pinyin": "bì",
    "description": "同煸，小火慢炒收干水分"
  },
  {
    "name": "煠",
    "pinyin": "yè",
    "description": "古技法，沸油或沸水速煮，如煠丸子"
  },
  {
    "name": "氽",
    "pinyin": "tǔn",
    "description": "油氽食材，如油氽花生米"
  },
  {
    "name": "溜",
    "pinyin": "liū",
    "description": "炸后裹酸甜芡汁，如溜肉段"
  },
  {
    "name": "扣",
    "pinyin": "kòu",
    "description": "食材排扣碗中蒸熟倒扣装盘，如梅菜扣肉"
  },
  {
    "name": "煲",
    "pinyin": "bāo",
    "description": "慢火久煮出浓汤，如煲汤"
  },
  {
    "name": "烫",
    "pinyin": "tàng",
    "description": "沸水收紧食材表皮，如烫猪皮"
  },
  {
    "name": "攒",
    "pinyin": "cuán",
    "description": "泼热油或酒增香，如攒酒烹虾"
  },
  {
    "name": "甑",
    "pinyin": "zèng",
    "description": "瓦钵装食材用强蒸汽蒸熟"
  },
  {
    "name": "啫",
    "pinyin": "zé",
    "description": "瓦煲极热烹煮出焦香，如啫啫鸡"
  },
  {
    "name": "飞",
    "pinyin": "fēi",
    "description": "即飞水，食材沸水速焯，如飞水去血沫"
  },
  {
    "name": "浸",
    "pinyin": "jìn",
    "description": "沸水或汤汁浸熟食材，如浸鸡"
  },
  {
    "name": "冰",
    "pinyin": "bīng",
    "description": "冰浸食材增脆，如冰浸苦瓜"
  },
  {
    "name": "拨",
    "pinyin": "bō",
    "description": "即拨丝，食材裹糖浆能拉丝，如拨丝苹果"
  },
  {
    "name": "挂",
    "pinyin": "guà",
    "description": "即挂霜，食材裹糖粉或糖浆，如挂霜花生"
  },
  {
    "name": "椒",
    "pinyin": "jiāo",
    "description": "椒盐翻炒炸熟食材，如椒盐排骨"
  },
  {
    "name": "油",
    "pinyin": "yóu",
    "description": "油泡，热油速熟食材，如油泡虾仁"
  },
  {
    "name": "走",
    "pinyin": "zǒu",
    "description": "走油，原料滚油速拖，如走油肉"
  },
  {
    "name": "火",
    "pinyin": "huǒ",
    "description": "火焰，白酒点燃热力烹熟，如火焰虾"
  },
  {
    "name": "串",
    "pinyin": "chuàn",
    "description": "串烧，食材穿串烤熟，如串烧牛肉"
  },
  {
    "name": "羹",
    "pinyin": "gēng",
    "description": "食材切丁煮后勾芡成糊状，如鸡蛋羹"
  },
  {
    "name": "灌",
    "pinyin": "guàn",
    "description": "馅料灌入皮料后蒸煮，如灌汤包"
  },
  {
    "name": "卷",
    "pinyin": "juǎn",
    "description": "食材裹馅料卷制后烹制，如春卷"
  },
  {
    "name": "酥",
    "pinyin": "sū",
    "description": "小火焖至骨酥肉烂，如酥鱼"
  },
  {
    "name": "煼",
    "pinyin": "chǎo",
    "description": "炒的古字，同炒，如煼青菜"
  },
  {
    "name": "炣",
    "pinyin": "kě",
    "description": "砂锅慢炖带汤汁，如炣豆腐"
  },
  {
    "name": "盐焗",
    "pinyin": "yán jú",
    "description": "粗盐加热焗熟，如盐焗鸡"
  },
  {
    "name": "炭",
    "pinyin": "tàn",
    "description": "炭烤，炭火直接烤熟，如炭烤生蚝"
  },
  {
    "name": "铁",
    "pinyin": "tiě",
    "description": "铁板烧，铁板加热烹制，如铁板烧牛肉"
  },
  {
    "name": "石",
    "pinyin": "shí",
    "description": "石烹，热石传热烹熟，如石烹鱼"
  },
  {
    "name": "筒",
    "pinyin": "tǒng",
    "description": "竹筒蒸，竹筒装食材蒸熟，如竹筒饭"
  },
  {
    "name": "蜜",
    "pinyin": "mì",
    "description": "蜜汁，糖蜜熬煮入味，如蜜汁叉烧"
  },
  {
    "name": "糖",
    "pinyin": "táng",
    "description": "糖煎，加糖慢煎收汁，如糖煎年糕"
  },
  {
    "name": "醋",
    "pinyin": "cù",
    "description": "醋溜，糖醋调味快炒，如醋溜白菜"
  },
  {
    "name": "酱卤",
    "pinyin": "jiàng lǔ",
    "description": "加酱料的卤制，如酱卤鸭"
  },
  {
    "name": "盐腌",
    "pinyin": "yán yān",
    "description": "盐腌制入味，如盐腌萝卜"
  },
  {
    "name": "湿熏",
    "pinyin": "shī xūn",
    "description": "用鲜花绍酒熏制，如湿熏鱼"
  },
  {
    "name": "烘培",
    "pinyin": "hōng bèi",
    "description": "低温烘烤面食，如烘焙蛋糕"
  },
  {
    "name": "炙",
    "pinyin": "zhì",
    "description": "传统明火直接烤肉，如炙肉"
  },
  {
    "name": "炒焙",
    "pinyin": "chǎo bèi",
    "description": "炒后烘干，如炒焙咖啡豆"
  },
  {
    "name": "香煎",
    "pinyin": "xiāng jiān",
    "description": "加香料煎制，如香煎鳕鱼"
  },
  {
    "name": "软炸",
    "pinyin": "ruǎn zhá",
    "description": "食材挂糊轻炸，如软炸里脊"
  },
  {
    "name": "粉蒸",
    "pinyin": "fěn zhēng",
    "description": "裹米粉蒸熟，如粉蒸肉"
  },
  {
    "name": "隔水炖",
    "pinyin": "gé shuǐ dùn",
    "description": "碗装食材隔水炖，如隔水炖燕窝"
  },
  {
    "name": "熬煮",
    "pinyin": "áo zhǔ",
    "description": "久煮至浓稠，如熬煮八宝粥"
  },
  {
    "name": "水汆",
    "pinyin": "shuǐ cuān",
    "description": "清水速烫，如水汆青菜"
  },
  {
    "name": "蒜炝",
    "pinyin": "suàn qiàng",
    "description": "蒜末炝拌，如蒜炝木耳"
  },
  {
    "name": "黄焖",
    "pinyin": "huáng mèn",
    "description": "黄酱焖煮，如黄焖鸡"
  },
  {
    "name": "炊",
    "pinyin": "chuī",
    "description": "烧火做饭的统称，如野炊、炊饭"
  },
  {
    "name": "爨",
    "pinyin": "cuàn",
    "description": "复杂的烧火做饭字，如爨灶烹菜"
  },
  {
    "name": "饪",
    "pinyin": "rèn",
    "description": "煮熟食物的统称，常与“烹”搭配为烹饪"
  },
  {
    "name": "烙",
    "pinyin": "lào",
    "description": "烧热器物压烫食材致熟，如烙饼、烙馍"
  },
  {
    "name": "燎",
    "pinyin": "liáo",
    "description": "旺火烈油快炒或燎去食材杂毛，如火燎鸭心"
  },
  {
    "name": "燔",
    "pinyin": "fán",
    "description": "古代明火直接烧烤，如燔肉"
  },
  {
    "name": "炀",
    "pinyin": "yáng",
    "description": "微火烘烤，如炀饼、炀面包片"
  },
  {
    "name": "炤",
    "pinyin": "zhào",
    "description": "明火照烤加热，如炤烤土豆"
  },
  {
    "name": "煓",
    "pinyin": "tuān",
    "description": "文火慢煮保温，如煓汤"
  },
  {
    "name": "熁",
    "pinyin": "xié",
    "description": "用热物烫熟食材，如熁鸡蛋"
  },
  {
    "name": "燖",
    "pinyin": "xún",
    "description": "沸水烫去食材表皮，如燖猪毛"
  },
  {
    "name": "熗",
    "pinyin": "qiàng",
    "description": "同“炝”，热油泼拌，如熗黄瓜"
  },
  {
    "name": "麩",
    "pinyin": "fū",
    "description": "用麸皮熏制，如麩熏鸡"
  },
  {
    "name": "酱焖",
    "pinyin": "jiàng mèn",
    "description": "酱焖带汤，如酱焖茄子"
  },
  {
    "name": "老卤",
    "pinyin": "lǎo lǔ",
    "description": "老卤慢浸，如老卤蛋"
  },
  {
    "name": "酸腌",
    "pinyin": "suān yān",
    "description": "酸腌，如酸腌菜"
  },
  {
    "name": "腊制",
    "pinyin": "là zhì",
    "description": "腊制风干，如腊香肠"
  },
  {
    "name": "高汤冻",
    "pinyin": "gāo tāng dòng",
    "description": "高汤冻"
  },
  {
    "name": "啫啫",
    "pinyin": "zé zé",
    "description": "啫啫牛蛙"
  },
  {
    "name": "盐水浸",
    "pinyin": "yán shuǐ jìn",
    "description": "盐水浸，如盐水浸鸡"
  },
  {
    "name": "冰醉",
    "pinyin": "bīng zuì",
    "description": "冰醉，如冰醉虾"
  },
  {
    "name": "拨丝",
    "pinyin": "bō sī",
    "description": "拨丝香蕉"
  },
  {
    "name": "炝",
    "pinyin": "qiàng",
    "description": "食材焯水后泼热油调料拌匀，如炝黄瓜"
  },
  {
    "name": "涮",
    "pinyin": "shuàn",
    "description": "薄切食材入沸汤速烫即食，如涮毛肚"
  },
  {
    "name": "熏",
    "pinyin": "xūn",
    "description": "烟火或香料熏制入味，如熏鸡"
  },
  {
    "name": "卤",
    "pinyin": "lǔ",
    "description": "食材放入卤汁慢煮入味，如卤牛肉"
  },
  {
    "name": "酱",
    "pinyin": "jiàng",
    "description": "用酱料焖煮使食材入味，如酱猪蹄"
  },
  {
    "name": "腌",
    "pinyin": "yān",
    "description": "调料浸泡食材入味，如腌黄瓜"
  },
  {
    "name": "糟",
    "pinyin": "zāo",
    "description": "食材放入酒糟中入味，如糟鸡"
  },
  {
    "name": "醉",
    "pinyin": "zuì",
    "description": "用烧酒浸泡食材，如醉虾"
  },
  {
    "name": "冻",
    "pinyin": "dòng",
    "description": "煮烂食材加琼脂冷藏凝结，如水晶冻"
  },
  {
    "name": "风",
    "pinyin": "fēng",
    "description": "腌制后通风阴干，如风干肉"
  },
  {
    "name": "腊",
    "pinyin": "là",
    "description": "腊月腌制后风干，如腊肉"
  },
  {
    "name": "拌",
    "pinyin": "bàn",
    "description": "生熟食材切后调拌，如凉拌黄瓜"
  },
  {
    "name": "糟焖",
    "pinyin": "zāo mèn",
    "description": "酒糟焖煮，如糟焖排骨"
  },
  {
    "name": "切",
    "pinyin": "qiē",
    "description": "食材切制预处理，如切丁切条"
  },
  {
    "name": "斩",
    "pinyin": "zhǎn",
    "description": "斩块预处理，如斩排骨"
  },
  {
    "name": "剁",
    "pinyin": "duò",
    "description": "剁成泥状，如剁肉馅"
  },
  {
    "name": "拍",
    "pinyin": "pāi",
    "description": "拍松食材易入味，如拍黄瓜"
  },
  {
    "name": "削",
    "pinyin": "xiāo",
    "description": "削去食材外皮，如削土豆"
  },
  {
    "name": "剥",
    "pinyin": "bō",
    "description": "剥去食材外壳，如剥虾"
  },
  {
    "name": "刮",
    "pinyin": "guā",
    "description": "刮去食材杂质，如刮鱼鳞"
  },
  {
    "name": "酿",
    "pinyin": "niàng",
    "description": "馅料填入食材，如酿豆腐"
  },
  {
    "name": "裹",
    "pinyin": "guǒ",
    "description": "食材裹粉或糊，如裹粉炸虾"
  },
  {
    "name": "淋",
    "pinyin": "lín",
    "description": "淋酱汁调味，如淋酱牛排"
  },
  {
    "name": "浇",
    "pinyin": "jiāo",
    "description": "浇热油或汤汁，如浇头面"
  },
  {
    "name": "蘸",
    "pinyin": "zhàn",
    "description": "食材蘸调料食用，如蘸料涮肉"
  },
  {
    "name": "炱",
    "pinyin": "tái",
    "description": "烟火熏制表层，如炱肉"
  }
];

export function cookingTextHtml(text, escapeHtml) {
  const methods = [...COOKING_METHODS].sort((a,b) => b.name.length-a.name.length);
  let html = "", offset = 0;
  while (offset < text.length) {
    const method = methods.find(m => text.startsWith(m.name,offset));
    if (method) {
      html += `<span class="cooking-method" title="${escapeHtml(`${method.name}（${method.pinyin}）：${method.description}`)}">${escapeHtml(method.name)}</span>`;
      offset += method.name.length;
    } else { html += escapeHtml(text[offset++]); }
  }
  return html;
}
